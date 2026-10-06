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

    def test_missing_ui_prepares_fixed_source_before_starting(self):
        from local_activity_monitor.ui_assets import MissingUIAssetsError
        with patch("local_activity_monitor.ui_assets.load_ui_assets", side_effect=MissingUIAssetsError("missing")), patch.object(launch.subprocess, "run", side_effect=[subprocess.CompletedProcess([], 0), subprocess.CompletedProcess([], 17)]) as run:
            self.assertEqual(launch.main(["--port", "8790"]), 17)
        self.assertEqual(run.call_args_list[0].args[0], [launch.sys.executable, "-I", "-B", str(self.root / "tools/prepare_ui.py"), "--ensure"])
        self.assertEqual(run.call_args_list[1].args[0][-2:], ["--port", "8790"])

    def test_failed_ui_preparation_does_not_start_service(self):
        from local_activity_monitor.ui_assets import MissingUIAssetsError
        with patch("local_activity_monitor.ui_assets.load_ui_assets", side_effect=MissingUIAssetsError("missing")), patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)) as run:
            self.assertEqual(launch.main([]), 1)
        self.assertEqual(run.call_count, 1)

    def test_invalid_existing_ui_is_preserved(self):
        with patch("local_activity_monitor.ui_assets.load_ui_assets", side_effect=ValueError("invalid checksum")), patch.object(launch.subprocess, "run") as run:
            self.assertEqual(launch.main([]), 1)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
