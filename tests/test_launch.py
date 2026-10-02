import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("monitor_launch", Path(__file__).resolve().parents[1] / "launch.py")
launch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launch)


class LaunchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="monitor launch ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.python = self.root / ".venv" / ("Scripts/python.exe" if launch.sys.platform == "win32" else "bin/python")
        self.file_patch = patch.object(launch, "__file__", str(self.root / "launch.py"))
        self.file_patch.start()
        self.addCleanup(self.file_patch.stop)

    def existing(self):
        self.python.parent.mkdir(parents=True)
        self.python.touch()

    @patch("builtins.input", return_value="n")
    @patch.object(launch.subprocess, "run")
    def test_decline_does_not_create_environment(self, run, prompt):
        self.assertEqual(launch.main([]), 0)
        run.assert_not_called()
        self.assertFalse((self.root / ".venv").exists())

    @patch("builtins.input", return_value="yes")
    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_first_setup_then_launch_preserves_arguments(self, run, prompt):
        self.assertEqual(launch.main(["--port", "8790"]), 0)
        calls = [call.args[0] for call in run.call_args_list]
        self.assertEqual(calls[0][-3:], ["-m", "venv", str(self.root / ".venv")])
        self.assertEqual(calls[1][-3:], ["install", "-r", "requirements.txt"])
        self.assertEqual(calls[2][-2:], ["--port", "8790"])
        self.assertTrue(all(call.kwargs["cwd"] == self.root for call in run.call_args_list))

    @patch("builtins.input")
    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_installed_skips_prompt_and_install(self, run, prompt):
        self.existing()
        self.assertEqual(launch.main(["--help"]), 0)
        prompt.assert_not_called()
        self.assertEqual(run.call_count, 2)
        self.assertNotIn("pip", str(run.call_args_list))

    @patch("builtins.input", return_value="y")
    @patch.object(launch.subprocess, "run")
    def test_install_failure_does_not_launch(self, run, prompt):
        run.side_effect = [subprocess.CompletedProcess([], 0), subprocess.CalledProcessError(1, ["pip"])]
        self.assertEqual(launch.main([]), 1)
        self.assertEqual(run.call_count, 2)

    @patch("builtins.input", return_value="y")
    @patch.object(launch.subprocess, "run")
    def test_missing_package_in_existing_environment_retries_install(self, run, prompt):
        self.existing()
        run.side_effect = [subprocess.CompletedProcess([], 1), subprocess.CompletedProcess([], 0), subprocess.CompletedProcess([], 0)]
        self.assertEqual(launch.main([]), 0)
        self.assertEqual(run.call_count, 3)
        self.assertEqual(run.call_args_list[1].args[0][-3:], ["install", "-r", "requirements.txt"])

    @patch("builtins.input")
    @patch.object(launch.subprocess, "run")
    def test_incomplete_environment_is_preserved(self, run, prompt):
        (self.root / ".venv").mkdir()
        self.assertEqual(launch.main([]), 1)
        run.assert_not_called()
        prompt.assert_not_called()
        self.assertTrue((self.root / ".venv").exists())


if __name__ == "__main__":
    unittest.main()
