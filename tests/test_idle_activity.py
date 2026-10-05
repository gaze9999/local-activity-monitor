from datetime import datetime
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from local_activity_monitor.idle_activity import IdleActivity
from local_activity_monitor.server import Dashboard


class IdleActivityTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.home = Path(self.directory.name)
        self.path = self.home/'sessions/rollout-fixture.jsonl'
        self.path.parent.mkdir()
        self.path.write_bytes(b'{}\n')
        self.collector = SimpleNamespace(files={self.path: {'offset': 3}}, next_scan=99)
        self.activity = IdleActivity(self.home)
        self.activity.check(self.collector, 5)
        self.activity.last_change -= 301

    def test_idle_keeps_same_metadata_and_exposes_no_paths(self):
        with patch.object(Path, 'open', side_effect=AssertionError('body read')):
            self.activity.check(self.collector, 5)
        self.assertTrue(self.activity.paused)
        value = self.activity.snapshot(5, 1)
        self.assertEqual(value['check_interval'], 10)
        self.assertEqual(self.activity.snapshot(5, 3600)['check_interval'], 30)
        self.assertNotIn(str(self.home), json.dumps(value))

    def test_appends_and_truncation_resume_and_scan_immediately(self):
        for content in (b'{}\n{}\n', b''):
            self.path.write_bytes(content)
            self.activity.check(self.collector, 5)
            self.assertFalse(self.activity.paused)
            self.assertIsNotNone(self.activity.last_activity_at)
            self.assertEqual(self.collector.next_scan, 0)
            self.activity.last_change -= 301

    def test_new_dated_conversation_is_detected_without_catalog(self):
        directory = self.home/'sessions'/datetime.now().strftime('%Y/%m/%d')
        directory.mkdir(parents=True)
        (directory/'rollout-new.jsonl').write_bytes(b'{}\n')
        self.activity.check(self.collector, 5, metadata=False)
        self.assertFalse(self.activity.paused)

    def test_catalog_wal_changes_are_detected_and_metadata_toggle_is_respected(self):
        path = self.home/'state_5.sqlite-wal'
        path.write_bytes(b'change')
        self.activity.check(self.collector, 5)
        self.assertFalse(self.activity.paused)
        self.activity.check(self.collector, 5, metadata=False)
        self.activity.last_change -= 301
        path.write_bytes(b'new change')
        self.activity.check(self.collector, 5, metadata=False)
        self.assertTrue(self.activity.paused)

    def test_disabled_and_pending_initialization_do_not_pause(self):
        for collector, minutes in ((None, 5), (self.collector, 0)):
            self.activity.check(collector, minutes)
            self.assertFalse(self.activity.paused)
            self.assertEqual(self.activity.health, 'disabled')
        self.collector.files[self.path]['history_cursor'] = 123
        self.activity.last_change -= 301
        self.activity.check(self.collector, 5)
        self.assertFalse(self.activity.paused)

    def test_permission_failure_or_metadata_limit_keeps_updates_running(self):
        with patch.object(Path, 'stat', side_effect=PermissionError()):
            self.activity.check(self.collector, 5)
        self.assertFalse(self.activity.paused)
        self.assertEqual(self.activity.health, 'unavailable')
        directory = self.home/'sessions'/datetime.now().strftime('%Y/%m/%d')
        directory.mkdir(parents=True)
        (directory/'rollout-new.jsonl').touch()
        with patch.object(self.activity, 'ENTRY_LIMIT', 0):
            self.activity.check(self.collector, 5)
        self.assertFalse(self.activity.paused)
        self.assertEqual(self.activity.health, 'unavailable')

    def test_dashboard_skips_collection_and_manual_resume_preserves_settings(self):
        app = Dashboard(self.home, codex=True)
        app.poll_once()
        app.activity.check(app.codex, 5)
        app.activity.last_change -= 301
        with patch.object(app, 'refresh', wraps=app.refresh) as refresh:
            self.assertFalse(app.poll_once())
            refresh.assert_not_called()
            self.assertTrue(app.snapshot('24h')['activity']['paused'])
            value = app.resume_refresh()
            self.assertFalse(value['paused'])
            refresh.assert_called_once_with()
        self.assertEqual(app.settings()['idle_minutes'], 5)
        for value in (-1, 1441, True, 5.5, '5'):
            with self.assertRaises(ValueError):
                app.set_settings({'idle_minutes': value})
        app.set_settings({'idle_minutes': 0})
        self.assertEqual(app.settings()['idle_minutes'], 0)
        app.set_settings(app.default_settings)
        self.assertEqual(app.settings()['idle_minutes'], 5)


if __name__ == '__main__':
    unittest.main()
