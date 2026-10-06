import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("monitor_launch", Path(__file__).resolve().parents[1] / "tools/launch-cli.py")
launch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launch)


class LaunchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="monitor launch ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.file_patch = patch.object(launch, "__file__", str(self.root / "tools/launch-cli.py"))
        self.file_patch.start()
        self.addCleanup(self.file_patch.stop)

    @patch("builtins.input")
    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_source_launch_never_installs_and_preserves_arguments(self, run, prompt):
        arguments = ["--codex-home", "資料夾 with spaces"]
        self.assertEqual(launch.main(arguments), 0)
        prompt.assert_not_called()
        run.assert_called_once_with([launch.sys.executable, "-I", "-B", str(self.root / "tools/watch.py"), "--codex", "--open", *arguments], cwd=self.root)
        self.assertFalse((self.root / ".venv").exists())

    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 17))
    def test_application_exit_code_is_preserved(self, run):
        self.assertEqual(launch.main([]), 17)

    @patch.object(launch.subprocess, "run", side_effect=OSError("missing interpreter"))
    def test_missing_interpreter_never_installs(self, run):
        self.assertEqual(launch.main([]), 1)
        run.assert_called_once()
        self.assertFalse((self.root / ".venv").exists())

    @patch.object(launch.sys, "version_info", (3, 9))
    @patch.object(launch.subprocess, "run")
    def test_unsupported_python_stops_before_launch(self, run):
        self.assertEqual(launch.main([]), 1)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
