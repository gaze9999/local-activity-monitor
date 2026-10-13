import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import MagicMock, patch

spec=importlib.util.spec_from_file_location("monitor_watch",Path(__file__).resolve().parents[1]/"tools/watch.py")
watch=importlib.util.module_from_spec(spec)
spec.loader.exec_module(watch)


class WatchTests(unittest.TestCase):
    def test_restart_preserves_arguments_and_does_not_open_another_tab(self):
        first,second=MagicMock(),MagicMock()
        first.process.poll.return_value=second.process.poll.return_value=None
        root=Path(__file__).resolve().parents[1]
        def replace(root,args,current,expected,stop):
            current.close();return second
        with patch.object(watch,"signature",side_effect=[("old",),("new",),("new",)]),patch.object(watch.time,"monotonic",side_effect=[0,4]),patch.object(watch,"build_frontend",return_value=0),patch.object(watch,"ServiceProcess",return_value=first) as spawn,patch.object(watch,"replace_service",side_effect=replace) as replacement,patch.object(watch.time,"sleep",side_effect=[None,KeyboardInterrupt]):
            self.assertEqual(watch.supervise(root,["--codex","--open","--port","8790"]),130)
        self.assertIn("--open",spawn.call_args.args[1])
        self.assertEqual(replacement.call_args.args[1][-2:],["--port","8790"])
        first.close.assert_called_once();second.close.assert_called_once()

    def test_startup_failure_is_reported_without_restart_loop(self):
        child=MagicMock();child.process.poll.return_value=1
        with patch.object(watch,"signature",return_value=()),patch.object(watch,"ServiceProcess",return_value=child) as spawn:
            self.assertEqual(watch.supervise(Path("."),[]),1)
        spawn.assert_called_once();child.close.assert_called_once()

    def test_forced_stop_only_targets_owned_child(self):
        child=MagicMock();child.poll.return_value=None
        child.stdin=None
        child.wait.side_effect=[subprocess.TimeoutExpired("owned-child",5),0]
        watch.stop_child(child)
        child.terminate.assert_called_once();child.kill.assert_called_once()

    def test_closed_control_pipe_does_not_replace_shutdown_result(self):
        child=MagicMock();child.poll.return_value=None
        child.stdin.write.side_effect=BrokenPipeError()
        child.stdin.close.side_effect=OSError(22,"Invalid argument")
        watch.stop_child(child)
        child.terminate.assert_called_once()
        child.wait.assert_called_once_with(timeout=15)
        child.stdin.close.assert_called_once()

    def test_signature_detects_source_change(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);source=root/"src/local_activity_monitor";source.mkdir(parents=True)
            path=source/"server.py";path.write_text("before")
            before=watch.signature(root);path.write_text("after change")
            self.assertNotEqual(watch.signature(root),before)

    def test_same_bytes_with_new_metadata_do_not_change_signature(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);source=root/"src/local_activity_monitor";source.mkdir(parents=True)
            path=source/"server.py";path.write_text("same bytes")
            before=watch.signature(root);metadata=path.stat()
            os.utime(path,ns=(metadata.st_atime_ns,metadata.st_mtime_ns+2000000000))
            self.assertEqual(watch.signature(root),before)
            path.write_text("new! bytes")
            os.utime(path,ns=(metadata.st_atime_ns,metadata.st_mtime_ns))
            self.assertNotEqual(watch.signature(root),before)

    def test_generated_loader_changes_do_not_schedule_another_source_build(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);bundle=root/"src/local_activity_monitor/_workbench";bundle.mkdir(parents=True)
            path=bundle/"__init__.py";path.write_text("old generated loader")
            before=watch.signature(root);path.write_text("rebuilt generated loader")
            self.assertEqual(watch.signature(root),before)
            (root/"workbench-ui.json").write_text('{}')
            self.assertNotEqual(watch.signature(root),before)

    def test_gui_stop_event_stops_only_owned_child(self):
        child=MagicMock();child.process.poll.return_value=None
        stop=MagicMock();stop.is_set.return_value=True
        with patch.object(watch,"signature",return_value=()),patch.object(watch,"ServiceProcess",return_value=child):
            self.assertEqual(watch.supervise(Path("."),[],stop),0)
        child.close.assert_called_once()

    def test_build_has_independent_input_latest_flag_and_bounded_wait(self):
        child=MagicMock();child.wait.return_value=7;child.poll.return_value=7
        root=Path("fixture")
        with patch.object(watch.subprocess,"Popen",return_value=child) as spawn:
            self.assertEqual(watch.build_frontend(root,latest=True),7)
        self.assertEqual(spawn.call_args.kwargs["stdin"],subprocess.DEVNULL)
        self.assertEqual(spawn.call_args.args[0][-1],"--latest")
        self.assertLessEqual(child.wait.call_args.kwargs["timeout"],.2)
        child.terminate.assert_not_called()

    def test_failed_update_keeps_server_and_does_not_repeat_unchanged_build(self):
        child=MagicMock();child.process.poll.return_value=None
        with patch.object(watch,"signature",side_effect=[("old",),("new",),("new",),("new",),("new",)]),patch.object(watch.time,"monotonic",side_effect=[0,4]),patch.object(watch,"build_frontend",return_value=124) as build,patch.object(watch,"ServiceProcess",return_value=child) as spawn,patch.object(watch.time,"sleep",side_effect=[None,None,KeyboardInterrupt]):
            self.assertEqual(watch.supervise(Path("."),[]),130)
        build.assert_called_once();spawn.assert_called_once()

    def test_invalid_prepared_source_never_pauses_healthy_service(self):
        current,candidate=MagicMock(),MagicMock();candidate.wait_for.return_value=False
        with patch.object(watch,"ServiceProcess",return_value=candidate):
            self.assertIs(watch.replace_service(Path("."),[],current,()),current)
        current.send.assert_not_called();current.close.assert_not_called();candidate.close.assert_called_once()

    def test_failed_replacement_resumes_same_service_and_closes_candidate_first(self):
        current,candidate=MagicMock(),MagicMock();current.process.poll.return_value=None
        current.wait_for.return_value=True;candidate.wait_for.side_effect=[True,False]
        calls=[];candidate.close.side_effect=lambda:calls.append("candidate closed")
        current.send.side_effect=lambda command:calls.append(command)
        with patch.object(watch,"ServiceProcess",return_value=candidate),patch.object(watch,"signature",return_value=()):
            self.assertIs(watch.replace_service(Path("."),[],current,()),current)
        self.assertEqual(calls,["pause","candidate closed","resume"])
        current.close.assert_not_called();candidate.send.assert_called_once_with("start")

    def test_successful_replacement_closes_previous_service_and_keeps_candidate(self):
        current,candidate=MagicMock(),MagicMock();candidate.wait_for.return_value=current.wait_for.return_value=True
        with patch.object(watch,"ServiceProcess",return_value=candidate) as spawn,patch.object(watch,"signature",return_value=()):
            self.assertIs(watch.replace_service(Path("."),["--open","--port","8790"],current,()),candidate)
        self.assertEqual(spawn.call_args.args[1],["--port","8790"])
        current.close.assert_called_once();candidate.close.assert_not_called()

    def test_actual_build_timeout_and_control_cancel_reap_owned_process(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/"tools").mkdir()
            script=root/"tools/build_frontend.py"
            script.write_text("import time\ntime.sleep(60)\n")
            real_spawn=subprocess.Popen
            processes=[]
            def spawn(*args,**kwargs):
                child=real_spawn(*args,**kwargs);processes.append(child);return child
            with patch.object(watch.subprocess,"Popen",side_effect=spawn):
                self.assertEqual(watch.build_frontend(root,timeout=.15),124)
                stop=threading.Event();timer=threading.Timer(.15,stop.set);timer.start()
                try:
                    self.assertEqual(watch.build_frontend(root,stop=stop),130)
                finally:
                    timer.cancel();timer.join()
            self.assertTrue(all(child.poll() is not None for child in processes))
            script.write_text("import sys\nassert sys.stdin.read() == ''\n")
            self.assertEqual(watch.build_frontend(root),0)


if __name__=="__main__":unittest.main()
