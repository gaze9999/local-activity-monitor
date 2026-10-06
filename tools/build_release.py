"""Build a native bundle on the matching OS and archive only release-owned files."""
from pathlib import Path
from importlib.metadata import distribution
import argparse
import os
import platform
import shutil
import subprocess
import sys
import tempfile

def main():
    parser = argparse.ArgumentParser(description="Build and test native bundles. Local output is test-only.")
    parser.add_argument("--release", action="store_true", help="Release workflow only")
    args = parser.parse_args()
    if args.release and os.environ.get("GITHUB_ACTIONS") != "true":
        parser.error("--release is restricted to GitHub Actions; omit it for a local test build")
    root = Path(__file__).resolve().parents[1]
    subprocess.run([sys.executable, str(root/"tools/prepare_ui.py")], cwd=root, check=True)
    sys.path.insert(0, str(root/"src"))
    from local_activity_monitor import __version__
    system = {"win32": "windows", "darwin": "macos", "linux": "linux"}[sys.platform]
    arch = "arm64" if platform.machine().lower() in ("arm64", "aarch64") else "x64"
    target = system+"-"+arch
    windows_icon = ["--icon", str(root/"src/local_activity_monitor/web/favicon.ico")] if system == "windows" else []
    if args.release:
        destination = root/"dist"
    else:
        tests = root/".local/package-tests"
        tests.mkdir(parents=True, exist_ok=True)
        destination = Path(tempfile.mkdtemp(prefix=target+"-", dir=tests))
    output = destination/target
    work = root/"build"/target if args.release else destination/"build"
    # Existing output may contain user files. Use a fresh release directory.
    output.mkdir(parents=True, exist_ok=False)
    subprocess.run([
        sys.executable, "-m", "PyInstaller", "--onedir", "--console", "--noupx",
        "--name", "launch-cli", "--paths", str(root/"src"),
        *windows_icon,
        "--add-data", str(root/"src/local_activity_monitor/web")+":local_activity_monitor/web",
        "--add-data", str(root/"src/local_activity_monitor/_workbench")+":local_activity_monitor/_workbench",
        "--hidden-import", "local_activity_monitor._workbench",
        "--distpath", str(output), "--workpath", str(work),
        "--specpath", str(work), str(root/"tools/launch-portable.py"),
    ], cwd=root, check=True)
    bundle = output/"launch-cli"
    shutil.move(str(bundle), str(output/"local-activity-monitor"))
    bundle = output/"local-activity-monitor"
    shutil.copy2(root/"README.md", bundle/"README.md")
    shutil.copytree(root/"docs", bundle/"docs")
    notices = bundle/"third-party-licenses"
    notices.mkdir()
    python_license = Path(sys.base_prefix)/"LICENSE.txt"
    if not python_license.is_file():
        python_license = Path(sys.base_prefix)/"lib"/("python"+platform.python_version()[:4])/"LICENSE.txt"
    if not python_license.is_file():
        raise FileNotFoundError("The bundled Python license must be included")
    shutil.copy2(python_license, notices/"Python-LICENSE.txt")
    installer = distribution("pyinstaller")
    for entry in installer.files or []:
        if "licenses" in entry.parts or entry.name.startswith(("COPYING", "LICENSE")):
            path = installer.locate_file(entry)
            if path.is_file():
                shutil.copy2(path, notices/("PyInstaller-"+entry.name))
    executable = bundle/("launch-cli.exe" if system == "windows" else "launch-cli")
    if system == "linux":
        launcher = bundle/"launch-cli.sh"
        launcher.write_text('#!/bin/sh\nROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1\nexec "$ROOT_DIR/launch-cli" "$@"\n', encoding="utf-8")
        launcher.chmod(0o755)
    subprocess.run([sys.executable, str(root/"tools/smoke_release.py"), str(executable)], check=True)
    if system == "windows":
        (bundle/"launch-cli.cmd").write_text('@echo off\n"%~dp0launch-cli.exe" %*\nexit /b %ERRORLEVEL%\n', encoding="utf-8")
        (bundle/"launch-cli.ps1").write_text("& (Join-Path $PSScriptRoot 'launch-cli.exe') @args\nexit $LASTEXITCODE\n", encoding="utf-8")
    elif system == "macos":
        launcher = bundle/"launch-cli.command"
        launcher.write_text('#!/bin/sh\nROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1\nexec "$ROOT_DIR/launch-cli" "$@"\n', encoding="utf-8")
        launcher.chmod(0o755)
    archive_format = "zip" if system == "windows" else "gztar"
    archive = shutil.make_archive(str(destination/("local-activity-monitor-"+__version__+"-"+target+"-cli")), archive_format, output, bundle.name)
    print(archive)


if __name__ == "__main__":
    main()
