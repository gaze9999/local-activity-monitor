import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import MagicMock, patch

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
        self.child = MagicMock()
        self.child.pid = 12345
        self.child.wait.return_value = 0
        self.child.poll.return_value = 0
        spawn = patch.object(launch.subprocess, "Popen", return_value=self.child)
        self.spawn = spawn.start()
        self.addCleanup(spawn.stop)

    @patch("builtins.input")
    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_source_launch_never_installs_and_preserves_arguments(self, run, prompt):
        arguments = ["--codex-home", "資料夾 with spaces"]
        self.assertEqual(launch.main(arguments), 0)
        prompt.assert_not_called()
        self.assertEqual(run.call_args_list[0].args[0], [launch.sys.executable, "-X", "utf8", "-I", "-B", str(self.root / "tools/build_frontend.py"), "--latest"])
        self.assertEqual(self.spawn.call_args.args[0], [launch.sys.executable, "-X", "utf8", "-I", "-B", str(self.root / "tools/watch.py"), "--watch-stdin", "--codex", "--open", *arguments])
        self.assertEqual(self.spawn.call_args.kwargs["stdin"], subprocess.PIPE)
        self.child.stdin.close.assert_called_once()
        self.assertFalse((self.root / ".venv").exists())

    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_application_exit_code_is_preserved(self, run):
        self.child.wait.return_value = 17
        self.assertEqual(launch.main([]), 17)

    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_status_records_listening_and_failure_without_private_output(self, run):
        self.child.stdout.__iter__.return_value = iter(['SECRET_PRIVATE_LOG\n', '{"status":"listening","url":"http://127.0.0.1:8790/","secret":"SECRET_KEY"}\n'])
        self.child.wait.return_value = 17
        self.assertEqual(launch.main(["--codex-home", "PRIVATE_PATH"]), 17)
        text = (self.root / ".local/startup-status.json").read_text()
        state = json.loads(text)
        self.assertEqual((state["stage"], state["result"], state["exit_code"]), ("watcher", "failed", 17))
        self.assertEqual(state["url"], "http://127.0.0.1:8790/")
        self.assertEqual(state["service_status"], "listening")
        self.assertNotIn("SECRET", text)
        self.assertNotIn("PRIVATE", text)
        self.child.stdout.close.assert_called_once()

    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_status_distinguishes_existing_service_and_rejects_external_url(self, run):
        self.child.stdout.__iter__.return_value = iter(['{"status":"listening","url":"https://example.invalid/"}\n', '{"status":"already_running","url":"http://127.0.0.1:8790/"}\n'])
        self.assertEqual(launch.main([]), 0)
        state = json.loads((self.root / ".local/startup-status.json").read_text())
        self.assertEqual((state["result"], state["exit_code"]), ("already_running", 0))
        self.assertEqual(state["url"], "http://127.0.0.1:8790/")

    def test_status_identifies_build_failure_before_watcher(self):
        with patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 7)):
            self.assertEqual(launch.main([]), 7)
        state = json.loads((self.root / ".local/startup-status.json").read_text())
        self.assertEqual((state["stage"], state["result"], state["exit_code"]), ("build_frontend", "failed", 7))
        self.spawn.assert_not_called()

    @patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0))
    def test_unwritable_status_does_not_prevent_startup(self, run):
        (self.root / ".local").write_text("existing user file")
        self.assertEqual(launch.main([]), 0)
        self.assertEqual((self.root / ".local").read_text(), "existing user file")

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

    def test_frontend_build_completes_before_starting(self):
        self.child.wait.return_value = 17
        with patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            self.assertEqual(launch.main(["--port", "8790"]), 17)
        self.assertEqual(run.call_args_list[0].args[0], [launch.sys.executable, "-X", "utf8", "-I", "-B", str(self.root / "tools/build_frontend.py"), "--latest"])
        self.assertEqual(self.spawn.call_args.args[0][-2:], ["--port", "8790"])

    def test_failed_ui_preparation_does_not_start_service(self):
        with patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)) as run:
            self.assertEqual(launch.main([]), 1)
        self.assertEqual(run.call_count, 1)
        self.spawn.assert_not_called()

    def test_invalid_existing_ui_is_preserved(self):
        with patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)) as run:
            self.assertEqual(launch.main([]), 1)
        run.assert_called_once()

    def test_interrupt_closes_owned_watcher_control_and_waits_for_shutdown(self):
        self.child.poll.return_value = None
        self.child.wait.side_effect = [KeyboardInterrupt, 0]
        self.child.stdin.close.side_effect = OSError(22, "Invalid argument")
        with patch.object(launch.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
            self.assertEqual(launch.main([]), 130)
        self.child.stdin.write.assert_called_once_with("restart\n")
        self.child.stdin.close.assert_called_once()
        self.child.terminate.assert_not_called()


if __name__ == "__main__":
    unittest.main()
