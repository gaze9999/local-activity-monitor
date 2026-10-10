from pathlib import Path
from contextlib import closing
import json
import sqlite3
import tempfile
import threading
import time
import unittest
from unittest.mock import Mock, patch

from local_activity_monitor.source_watch import SourceWatch, source_path
from local_activity_monitor.server import Dashboard


class SourceWatchTests(unittest.TestCase):
    def test_source_selection_excludes_secrets_and_owned_writes(self):
        for name in ('sessions/2026/new.jsonl', 'sqlite/codex-dev.db-wal', 'state_5.sqlite-wal',
                     'monitoring/jev-monitor.json', 'desktop-logs/trace.log', 'config.toml'):
            self.assertTrue(source_path(name), name)
        for name in ('auth.json', 'monitoring/lam-history.sqlite3-wal', 'monitoring/thread-state.json',
                     'monitoring/local-activity-monitor.jsonl', 'monitoring/performance-debug.jsonl',
                     'monitoring/performance-debug.jsonl.3', '../outside', '.cache/file', 'state_5.sqlite-shm'):
            self.assertFalse(source_path(name), name)

    def test_native_append_create_rename_delete_and_cancel_without_polling(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'sessions').mkdir()
            changed=threading.Event()
            watch=SourceWatch(root, changed.set, source_path).start()
            try:
                self.assertTrue(watch.ready.wait(5))
                self.assertEqual(watch.health, 'ok', watch.error_type)
                ignored=root/'monitoring'
                ignored.mkdir()
                # Creating the monitored config directory can request one rescan.
                changed.wait(.1);changed.clear()
                (ignored/'lam-history.sqlite3').write_bytes(b'owned state')
                self.assertFalse(changed.wait(.2))
                (ignored/'lam-history.sqlite3').unlink()
                changed.clear()
                for index in range(3):
                    with closing(sqlite3.connect(ignored/'lam-history.sqlite3')) as db, db:
                        db.execute('CREATE TABLE IF NOT EXISTS owned(value)')
                        db.execute('INSERT INTO owned VALUES(?)',(index,))
                    self.assertFalse(changed.wait(.2),'Owned SQLite journal must not trigger collection')
                debug = ignored/'performance-debug.jsonl'
                debug.write_bytes(b'{}\n')
                with debug.open('ab') as stream:
                    stream.write(b'{}\n')
                debug.rename(ignored/'performance-debug.jsonl.1')
                self.assertFalse(changed.wait(.2),'Debug writes and rotation must not trigger collection')
                path=root/'sessions/event.jsonl'
                for action in (lambda:path.write_bytes(b'{}\n'), lambda:path.write_bytes(b'{}\n{}\n'),
                               lambda:path.rename(root/'sessions/renamed.jsonl'),
                               lambda:(root/'sessions/renamed.jsonl').unlink()):
                    changed.clear();action()
                    self.assertTrue(changed.wait(3))
                changed.clear()
                deadline = time.monotonic()+1
                while changed.wait(.1) and time.monotonic() < deadline:
                    changed.clear()
                self.assertFalse(changed.wait(.2))
            finally:
                watch.close()
            self.assertFalse(watch.thread.is_alive())

    def test_dashboard_waits_for_changes_and_wakes_during_collection(self):
        with tempfile.TemporaryDirectory() as directory:
            app=Dashboard(Path(directory),codex=False)
            collected=threading.Event()
            calls=[]
            def collect():
                calls.append(1)
                if len(calls)==2:
                    app.source_changed.set()
                collected.set()
            with patch.object(app,'watch_sources'), patch.object(app,'close_sources'), patch.object(app,'poll_once',side_effect=collect):
                app.thread.start()
                try:
                    self.assertTrue(collected.wait(3));collected.clear()
                    self.assertFalse(collected.wait(.2))
                    app.source_changed.set()
                    self.assertTrue(collected.wait(3))
                    deadline=time.monotonic()+3
                    while len(calls)<3 and time.monotonic()<deadline:
                        collected.wait(.01)
                    self.assertEqual(len(calls),3)
                finally:
                    app.stop.set();app.source_changed.set();app.thread.join(3)
                self.assertFalse(app.thread.is_alive())

    def test_legacy_interval_is_accepted_without_scheduling_or_export(self):
        with tempfile.TemporaryDirectory() as directory:
            app=Dashboard(Path(directory),codex=False,interval=1)
            with patch.object(app,'refresh'):
                value=app.set_settings({'interval':3600})
            self.assertNotIn('interval',value)
            self.assertNotIn('interval',app.default_settings)
            self.assertNotIn('check_interval',app.activity_status())
            self.assertTrue(app.source_changed.is_set())
            self.assertTrue(app.retry_sources.is_set())

    def test_external_jev_selection_and_explicit_failed_watch_rearm(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            app=Dashboard(root/'home')
            app.home.mkdir(exist_ok=True)
            database=root/'jev.sqlite3'
            failed=Mock(health='unavailable')
            healthy=Mock(health='ok')
            watches=[Mock(health='ok'),failed,healthy]
            for watch in watches:
                watch.start.return_value=watch
            with patch('local_activity_monitor.server.monitor_config',return_value=(True,database,{})), \
                 patch('local_activity_monitor.server.SourceWatch',side_effect=watches) as factory:
                try:
                    app.watch_sources()
                    selected=factory.call_args.args[2]
                    self.assertTrue(selected(Path('jev.sqlite3-wal')))
                    self.assertFalse(selected(Path('jev.sqlite3-shm')))
                    self.assertFalse(selected(Path('unrelated.sqlite3')))
                    app.watch_sources()
                    self.assertEqual(factory.call_count,2)
                    with patch.object(app,'refresh'):
                        app.resume_refresh()
                    app.watch_sources()
                    self.assertEqual(factory.call_count,3)
                    failed.close.assert_called_once()
                    app.observations['jev']=False
                    app.watch_sources()
                    healthy.close.assert_called_once()
                    self.assertNotIn(database.parent,app.source_watches)
                finally:
                    app.close_sources()

    def test_pending_backfill_continues_without_more_source_events(self):
        with tempfile.TemporaryDirectory() as directory:
            app=Dashboard(Path(directory),codex=True)
            state={'offset':0, 'partial_history':True}
            app.codex.files={Path(directory)/'sessions/fixture.jsonl':state}
            completed=threading.Event()
            def collect():
                state['offset']+=1
                state['partial_history']=state['offset']<3
                if not state['partial_history']:
                    completed.set()
            with patch.object(app,'watch_sources'),patch.object(app,'close_sources'),patch.object(app,'poll_once',side_effect=collect):
                app.thread.start()
                try:
                    self.assertTrue(completed.wait(3))
                    self.assertEqual(state['offset'],3)
                finally:
                    app.stop.set();app.source_changed.set();app.thread.join(3)


if __name__=='__main__':
    unittest.main()
