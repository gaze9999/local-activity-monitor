import importlib.util
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("monitor_release", ROOT / "tools/build_release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseAssemblyTests(unittest.TestCase):
    def test_local_build_cannot_use_release_mode(self):
        with patch.dict(os.environ, {"GITHUB_ACTIONS": "false"}), patch.object(sys, "argv", ["build_release.py", "--release"]), self.assertRaises(SystemExit) as result:
            release.main()
        self.assertEqual(result.exception.code, 2)

    def test_independent_windows_and_macos_test_archives(self):
        for platform in ("win32", "darwin", "linux"):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as folder:
                root = Path(folder).resolve()
                (root / "tools").mkdir()
                (root / "docs").mkdir()
                (root / "runtime").mkdir()
                (root / "runtime/LICENSE.txt").write_text("fixture license")
                (root / "README.md").write_text("fixture readme")
                for name in ("LICENSE", "NOTICE"):
                    (root / name).write_text("fixture " + name)
                (root / "docs/usage.md").write_text("fixture usage")
                for name in ("launch-cli.cmd", "tools/launch-windows.ps1"):
                    (root / name).write_text("fixture launcher")
                calls = []
                def run(command, **kwargs):
                    calls.append(command)
                    if "PyInstaller" not in command:
                        return Mock(returncode=0)
                    output = Path(command[command.index("--distpath") + 1])
                    name = command[command.index("--name") + 1]
                    if platform == "win32":
                        self.assertEqual(command[command.index("--icon")+1], str(root / "src/local_activity_monitor/web/favicon.ico"))
                    else:
                        self.assertNotIn("--icon", command)
                    self.assertEqual(name, "launch-cli")
                    self.assertIn("--console", command)
                    self.assertNotIn("webview", str(command))
                    bundle = output / name
                    bundle.mkdir(parents=True)
                    (bundle / "_internal").mkdir()
                    (bundle / (name + (".exe" if platform == "win32" else ""))).write_text("fixture binary")
                with patch.object(release, "__file__", str(root / "tools/build_release.py")), patch.object(sys, "argv", ["build_release.py"]), patch.object(release.sys, "platform", platform), patch.object(release.sys, "base_prefix", str(root / "runtime")), patch.object(release.platform, "machine", return_value="x86_64"), patch.object(release, "distribution", return_value=Mock(files=[])), patch.object(release.subprocess, "run", side_effect=run):
                    release.main()
                self.assertFalse((root / "dist").exists())
                bundle = next((root / ".local/package-tests").glob("*/windows-x64/local-activity-monitor")) if platform == "win32" else next((root / ".local/package-tests").glob("*/*/local-activity-monitor"))
                for name in ("LICENSE", "NOTICE"):
                    self.assertEqual((bundle / name).read_text(), "fixture " + name)
                if platform == "win32":
                    cli_archive = next((root / ".local/package-tests").rglob("*-cli.zip"))
                    with zipfile.ZipFile(cli_archive) as package:
                        cli_names = package.namelist()
                    self.assertIn("local-activity-monitor/launch-cli.cmd", cli_names)
                    self.assertIn("local-activity-monitor/launch-cli.ps1", cli_names)
                    self.assertIn("local-activity-monitor/launch-cli.exe", cli_names)
                elif platform == "darwin":
                    cli_archive = next((root / ".local/package-tests").rglob("*-cli.tar.gz"))
                    with tarfile.open(cli_archive) as package:
                        cli_names = package.getnames()
                    self.assertIn("local-activity-monitor/launch-cli.command", cli_names)
                    self.assertIn("local-activity-monitor/launch-cli", cli_names)
                    self.assertFalse(any(".app/" in name for name in cli_names))
                    self.assertFalse(any(command[0] == "codesign" for command in calls))
                else:
                    archive = next((root / ".local/package-tests").rglob("*.tar.gz"))
                    with tarfile.open(archive) as package:
                        names = package.getnames()
                    self.assertIn("local-activity-monitor/launch-cli.sh", names)
                    self.assertIn("local-activity-monitor/launch-cli", names)
                self.assertFalse(any("--smoke-test" in command for command in calls))
                self.assertEqual(sum("PyInstaller" in command for command in calls), 1)


if __name__ == "__main__":
    unittest.main()
