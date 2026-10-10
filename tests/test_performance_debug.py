import json
import ctypes
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.performance_debug import PerformanceDebug, process_memory
from local_activity_monitor.server import Dashboard


class PerformanceDebugTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)/'monitoring/performance-debug.jsonl'
        self.debug = PerformanceDebug(self.path)

    def test_disabled_creates_no_files_and_settings_validate(self):
        self.debug.record('collection', {'cpu_ms': 1})
        self.assertFalse(self.path.exists())
        self.assertEqual(self.debug.snapshot()['records'], [])
        app = Dashboard(Path(self.temp.name))
        self.assertFalse(app.settings()['debug_mode'])
        for value in (1, 'true', None):
            with self.assertRaises(ValueError):
                app.set_settings({'debug_mode': value})

    def test_records_numeric_metrics_without_private_content(self):
        self.debug.set_enabled(True)
        self.debug.record('collection', {'cpu_ms': 1.5, 'refresh_ms': 20, 'message': 'PRIVATE_PAYLOAD', 'path': 'PRIVATE_PATH',
            'read_bytes': float('inf'), 'buffer_bytes': True, 'phase_metrics': {'sessions': {'cpu_ms': 1, 'message': 'PRIVATE_PHASE'}, 'PRIVATE_NAME': {'cpu_ms': 1}}})
        self.debug.set_enabled(False)
        saved = self.path.read_bytes()
        self.debug.record('collection', {'cpu_ms': 100})
        self.assertEqual(saved, self.path.read_bytes())
        self.assertNotIn(b'PRIVATE', saved)
        record = self.debug.snapshot()['records'][1]
        self.assertEqual(record['cpu_ms'], 1.5)
        self.assertNotIn('read_bytes', record)
        self.assertNotIn('buffer_bytes', record)
        self.assertEqual(record['phase_metrics'], {'sessions': {'cpu_ms': 1}})

    def test_rotation_restart_and_memory_are_bounded(self):
        self.debug.FILE_BYTES = 4096
        self.debug.set_enabled(True)
        for index in range(200):
            self.debug.record('collection', {'cpu_ms': index})
        files = list(self.path.parent.glob('performance-debug.jsonl*'))
        self.assertEqual(len(files), 4)
        self.assertTrue(all(path.stat().st_size <= 4096 for path in files))
        self.assertEqual(len(self.debug.snapshot()['records']), 150)
        restarted = PerformanceDebug(self.path)
        latest = restarted.snapshot()['records'][0]
        self.assertEqual(latest['cpu_ms'], 199)
        self.assertFalse(restarted.enabled)
        self.assertLessEqual(len(restarted.records), 150)

    def test_invalid_journal_and_unsafe_paths_are_preserved(self):
        self.path.parent.mkdir()
        self.path.write_text('PRIVATE_UNRELATED_FILE', encoding='utf-8')
        self.debug.set_enabled(True)
        self.assertEqual(self.path.read_text(), 'PRIVATE_UNRELATED_FILE')
        self.assertEqual(self.debug.snapshot()['health'], 'unavailable')
        self.path.unlink()
        debug = PerformanceDebug(self.path)
        with patch('local_activity_monitor.performance_debug.unsafe', return_value=True):
            debug.set_enabled(True)
        self.assertFalse(self.path.exists())
        self.assertEqual(debug.snapshot()['health'], 'unavailable')

    def test_write_failure_does_not_interrupt_collection(self):
        self.debug.set_enabled(True)
        with patch.object(self.debug, 'update_files', side_effect=PermissionError):
            self.debug.record('collection', {'cpu_ms': 2})
        self.assertEqual(self.debug.snapshot()['error_type'], 'PermissionError')
        self.assertEqual(self.debug.snapshot()['records'][0]['cpu_ms'], 2)

    def test_future_journal_format_is_preserved(self):
        self.path.parent.mkdir()
        raw = json.dumps({'application': 'local-activity-monitor', 'version': 2, 'kind': 'collection', 'cpu_ms': 1})+'\n'
        self.path.write_text(raw, encoding='utf-8')
        self.debug.set_enabled(True)
        self.assertEqual(self.path.read_text(encoding='utf-8'), raw)
        self.assertEqual(self.debug.snapshot()['health'], 'unavailable')
        self.assertFalse(any(record['kind'] == 'collection' for record in self.debug.records))

    def test_process_memory_is_numeric_when_available(self):
        for value in process_memory().values():
            if value is not None:
                self.assertGreater(value, 0)

    def test_native_memory_samples_do_not_accumulate_pointer_types(self):
        process_memory()
        cache = getattr(ctypes, '_pointer_type_cache', None)
        if cache is None:
            self.skipTest('CPython ctypes cache unavailable')
        before = len(cache)
        for _ in range(100):
            process_memory()
        self.assertEqual(len(cache), before)


if __name__ == '__main__':
    unittest.main()
