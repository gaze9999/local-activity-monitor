from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.monitor_usage import MonitorUsage


class MonitorUsageTests(unittest.TestCase):
    def test_samples_only_owned_files_and_retains_unknown_memory(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            owned, unrelated = base/'lam.sqlite', base/'private-source'
            owned.write_bytes(b'12345')
            unrelated.write_bytes(b'x'*100)
            usage = MonitorUsage()
            usage.paths = (owned, owned, base/'missing')
            with patch('local_activity_monitor.monitor_usage.process_memory', return_value={'rss_bytes':None, 'peak_rss_bytes':None}):
                value = usage.sample()
            self.assertEqual(value['disk_bytes'], 5)
            self.assertEqual(value['storage_files'], 1)
            self.assertIsNone(value['rss_bytes'])
            self.assertEqual(value['storage_health'], 'ok')
            value['disk_bytes'] = 999
            self.assertEqual(usage.sample()['disk_bytes'], 5)

    def test_cpu_sample_is_process_time_and_can_use_multiple_cores(self):
        usage = MonitorUsage()
        usage.previous = (10, 2)
        with patch('local_activity_monitor.monitor_usage.time.monotonic', return_value=12), patch('local_activity_monitor.monitor_usage.time.process_time', return_value=5):
            self.assertEqual(usage.sample()['cpu_percent'], 150)
