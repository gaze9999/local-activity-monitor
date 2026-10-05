import io
import unittest
from unittest.mock import patch

from local_activity_monitor.device_info import CpuUsage, cpu_times, memory_info


class DeviceInfoTests(unittest.TestCase):
    def test_cpu_delta_idle_and_reset(self):
        with patch('local_activity_monitor.device_info.cpu_times', side_effect=[(10, 100), (20, 140), (20, 140), (1, 10), None, (2, 20)]):
            usage = CpuUsage()
            self.assertEqual(usage.sample(), 75)
            self.assertIsNone(usage.sample())
            self.assertIsNone(usage.sample())
            self.assertIsNone(usage.sample())
            self.assertIsNone(usage.sample())

    def test_linux_uses_first_eight_fields_without_double_counting_guest(self):
        with patch('local_activity_monitor.device_info.sys.platform', 'linux'), patch('pathlib.Path.open', return_value=io.StringIO('cpu 10 20 30 40 5 6 7 8 100 200\n')):
            self.assertEqual(cpu_times(), (45, 126))

    def test_unavailable_counters_and_missing_available_memory(self):
        with patch('local_activity_monitor.device_info.sys.platform', 'linux'), patch('pathlib.Path.open', side_effect=OSError):
            self.assertIsNone(cpu_times())
            self.assertEqual(memory_info(), {'physical_memory_bytes': None, 'available_memory_bytes': None})
        with patch('local_activity_monitor.device_info.sys.platform', 'linux'), patch('pathlib.Path.open', return_value=io.StringIO('MemTotal: 1000 kB\n')):
            self.assertEqual(memory_info()['physical_memory_bytes'], 1024000)
            self.assertIsNone(memory_info()['available_memory_bytes'])

    def test_processor_group_scope(self):
        with patch('local_activity_monitor.device_info.sys.platform', 'win32'), patch('local_activity_monitor.device_info.os.cpu_count', return_value=128), patch('local_activity_monitor.device_info.cpu_times', return_value=None):
            self.assertEqual(CpuUsage().scope, 'processor_group')
