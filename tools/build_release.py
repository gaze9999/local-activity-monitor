"""Build a native bundle on the matching OS and archive only release-owned files."""
from pathlib import Path
from importlib.metadata import distribution
import platform
import shutil
import subprocess
import sys


def main():
    root = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(root/"src"))
    from local_activity_monitor import __version__
    system = {"win32": "windows", "darwin": "macos", "linux": "linux"}[sys.platform]
    arch = "arm64" if platform.machine().lower() in ("arm64", "aarch64") else "x64"
    target = system+"-"+arch
    output = root/"dist"/target
    # Existing output may contain user files. Use a fresh release directory.
    output.mkdir(parents=True, exist_ok=False)
    subprocess.run([
        sys.executable, "-m", "PyInstaller", "--onedir", "--console", "--noupx",
        "--name", "local-activity-monitor", "--paths", str(root/"src"),
        "--add-data", str(root/"src/local_activity_monitor/web")+":local_activity_monitor/web",
        "--distpath", str(output), "--workpath", str(root/"build"/target),
        "--specpath", str(root/"build"/target), str(root/"tools/portable.py"),
    ], cwd=root, check=True)
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
    if system == "windows":
        (bundle/"Start.cmd").write_text('@echo off\r\n"%~dp0local-activity-monitor.exe" %*\r\nif errorlevel 1 pause\r\n', encoding="utf-8", newline="")
    else:
        launcher = bundle/("Start.command" if system == "macos" else "start.sh")
        launcher.write_text('#!/bin/sh\nROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1\nexec "$ROOT_DIR/local-activity-monitor" "$@"\n', encoding="utf-8")
        launcher.chmod(0o755)
    executable = bundle/("local-activity-monitor.exe" if system == "windows" else "local-activity-monitor")
    subprocess.run([sys.executable, str(root/"tools/smoke_release.py"), str(executable)], check=True)
    archive_format = "zip" if system == "windows" else "gztar"
    archive = shutil.make_archive(str(root/"dist"/("local-activity-monitor-"+__version__+"-"+target)), archive_format, output, bundle.name)
    print(archive)


if __name__ == "__main__":
    main()
