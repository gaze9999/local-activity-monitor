import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import MagicMock, patch

spec=importlib.util.spec_from_file_location("monitor_watch",Path(__file__).resolve().parents[1]/"tools/watch.py")
watch=importlib.util.module_from_spec(spec)
spec.loader.exec_module(watch)


class WatchTests(unittest.TestCase):
    def test_restart_preserves_arguments_and_does_not_open_another_tab(self):
        first,second=MagicMock(),MagicMock()
        first.poll.return_value=second.poll.return_value=None
        root=Path(__file__).resolve().parents[1]
        with patch.object(watch,"signature",side_effect=[("old",),("new",),("new",)]),patch.object(watch.time,"monotonic",side_effect=[0,4]),patch.object(watch.subprocess,"Popen",side_effect=[first,second]) as spawn,patch.object(watch.time,"sleep",side_effect=[None,KeyboardInterrupt]):
            self.assertEqual(watch.supervise(root,["--codex","--open","--port","8790"]),130)
        self.assertIn("--open",spawn.call_args_list[0].args[0])
        self.assertNotIn("--open",spawn.call_args_list[1].args[0])
        self.assertEqual(spawn.call_args_list[1].args[0][-2:],["--port","8790"])
        first.stdin.write.assert_called_once_with("restart\n");second.stdin.write.assert_called_once_with("restart\n")

    def test_startup_failure_is_reported_without_restart_loop(self):
        child=MagicMock();child.poll.return_value=1
        with patch.object(watch,"signature",return_value=()),patch.object(watch.subprocess,"Popen",return_value=child) as spawn:
            self.assertEqual(watch.supervise(Path("."),[]),1)
        spawn.assert_called_once();child.terminate.assert_not_called()

    def test_forced_stop_only_targets_owned_child(self):
        child=MagicMock();child.poll.return_value=None
        child.stdin=None
        child.wait.side_effect=[subprocess.TimeoutExpired("owned-child",5),0]
        watch.stop_child(child)
        child.terminate.assert_called_once();child.kill.assert_called_once()

    def test_signature_detects_source_change(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);source=root/"src/local_activity_monitor";source.mkdir(parents=True)
            path=source/"server.py";path.write_text("before")
            before=watch.signature(root);path.write_text("after change")
            self.assertNotEqual(watch.signature(root),before)

    def test_gui_stop_event_stops_only_owned_child(self):
        child=MagicMock();child.poll.return_value=None
        stop=MagicMock();stop.is_set.return_value=True
        with patch.object(watch,"signature",return_value=()),patch.object(watch.subprocess,"Popen",return_value=child):
            self.assertEqual(watch.supervise(Path("."),[],stop),0)
        child.stdin.write.assert_called_once_with("restart\n")
        child.wait.assert_called_once_with(timeout=5)
        child.terminate.assert_not_called()


if __name__=="__main__":unittest.main()
