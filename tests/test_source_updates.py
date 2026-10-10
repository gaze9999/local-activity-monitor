from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.server import Dashboard


class SourceUpdateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.app = Dashboard(Path(self.temp.name))

    def test_legacy_timers_are_validated_and_ignored(self):
        with patch.object(self.app, 'refresh'):
            self.app.set_settings({'interval': 1, 'idle_minutes': 5})
        for settings in (self.app.settings(), self.app.default_settings):
            self.assertNotIn('interval', settings)
            self.assertNotIn('idle_minutes', settings)
        self.assertFalse(self.app.activity_status()['paused'])
        for minutes in (-1, 1441, True, 5.5, '5'):
            with self.subTest(minutes=minutes), self.assertRaises(ValueError):
                self.app.set_settings({'idle_minutes': minutes})

    def test_collector_wait_has_no_idle_deadline(self):
        with patch.object(self.app, 'watch_sources'), patch.object(self.app.source_changed, 'wait', return_value=True) as wait, \
                patch.object(self.app, 'poll_once', side_effect=self.app.stop.set):
            self.app.poll()
        wait.assert_called_once_with()

    def test_source_event_collects_without_activity_scan(self):
        with patch.object(self.app, 'refresh') as refresh:
            self.assertTrue(self.app.poll_once())
        refresh.assert_called_once_with(stagger=True)
        self.assertIsNotNone(self.app.activity_status()['last_activity_at'])


if __name__ == '__main__':
    unittest.main()
