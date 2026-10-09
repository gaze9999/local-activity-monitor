import ctypes
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

from local_activity_monitor.gpu_info import gpu_info


class GPUInfoTests(unittest.TestCase):
    def fixture(self, descriptions, driver_error=False):
        released = []
        def create(guid, output):
            output._obj.value = 1
            return 0
        def member(pointer, index, result, *arguments):
            if index == 2:
                return lambda pointer: released.append(pointer.value)
            if index == 12:
                def enumerate_adapter(factory, position, output):
                    if position >= len(descriptions):
                        return -1
                    output._obj.value = position+2
                    return 0
                return enumerate_adapter
            if index == 10:
                def describe(adapter, output):
                    name, memory, shared, flags = descriptions[adapter.value-2]
                    output._obj.name = name
                    output._obj.dedicated, output._obj.shared, output._obj.flags = memory, shared, flags
                    return 0
                return describe
            if index == 9:
                def driver(adapter, guid, output):
                    output._obj.value = (32 << 48) | (1 << 32) | (101 << 16) | 7076
                    return -1 if driver_error else 0
                return driver
            raise AssertionError(index)
        return Mock(CreateDXGIFactory1=Mock(side_effect=create)), member, released

    def test_multiple_adapters_preserve_64_bit_capacity_and_skip_software(self):
        library, member, released = self.fixture([
            ('Integrated GPU', 128*1024**2, 8*1024**3, 0),
            ('Dedicated GPU', 24*1024**3, 8*1024**3, 0),
            ('Software GPU', 0, 0, 2)])
        with patch('local_activity_monitor.gpu_info.sys.platform', 'win32'), patch.object(ctypes, 'WinDLL', return_value=library, create=True), patch('local_activity_monitor.gpu_info._method', side_effect=member):
            result = gpu_info()
        self.assertEqual(len(result), 2)
        self.assertEqual(result[1]['dedicated_memory_bytes'], 24*1024**3)
        self.assertEqual(result[0]['shared_memory_bytes'], 8*1024**3)
        self.assertEqual(result[0]['driver_version'], '32.1.101.7076')
        self.assertEqual(released, [2, 3, 4, 1])
        self.assertEqual(set(result[0]), {'name', 'dedicated_memory_bytes', 'shared_memory_bytes', 'driver_version'})

    def test_unavailable_driver_keeps_capacity_and_zero_is_valid(self):
        library, member, released = self.fixture([('GPU', 0, 8*1024**3, 0)], driver_error=True)
        with patch('local_activity_monitor.gpu_info.sys.platform', 'win32'), patch.object(ctypes, 'WinDLL', return_value=library, create=True), patch('local_activity_monitor.gpu_info._method', side_effect=member):
            result = gpu_info()[0]
        self.assertEqual(result['dedicated_memory_bytes'], 0)
        self.assertIsNone(result['driver_version'])
        self.assertEqual(released, [2, 1])

    def test_unsupported_platform_and_failed_library_are_unknown(self):
        with patch('local_activity_monitor.gpu_info.sys.platform', 'unsupported'), patch.object(ctypes, 'WinDLL', side_effect=AssertionError('unexpected native call'), create=True):
            self.assertEqual(gpu_info(), [])
        with patch('local_activity_monitor.gpu_info.sys.platform', 'win32'), patch.object(ctypes, 'WinDLL', side_effect=OSError(), create=True):
            self.assertEqual(gpu_info(), [])

    def test_adapter_enumeration_is_bounded_and_releases_resources(self):
        library, member, released = self.fixture([('GPU', 4*1024**3, 0, 0)]*20)
        with patch('local_activity_monitor.gpu_info.sys.platform', 'win32'), patch.object(ctypes, 'WinDLL', return_value=library, create=True), patch('local_activity_monitor.gpu_info._method', side_effect=member):
            self.assertEqual(len(gpu_info()), 16)
        self.assertEqual(len(released), 17)

    def test_mac_unified_and_shared_memory_are_not_dedicated_vram(self):
        payload = {'SPDisplaysDataType': [
            {'_name': 'Apple M4', 'spdisplays_vram': '32 GB', '_spdisplays_display': [{'serial': 'PRIVATE_SERIAL'}]},
            {'_name': 'AMD GPU', 'spdisplays_vram': '8 GB'},
            {'_name': 'Intel GPU', 'spdisplays_vram_shared': '1536 MB'}]}
        with patch('local_activity_monitor.gpu_info.sys.platform', 'darwin'), patch('local_activity_monitor.gpu_info._command', return_value=json.dumps(payload)):
            result = gpu_info()
        self.assertEqual(result[0]['memory_type'], 'unified')
        self.assertIsNone(result[0]['dedicated_memory_bytes'])
        self.assertEqual(result[1]['dedicated_memory_bytes'], 8*1024**3)
        self.assertIsNone(result[2]['dedicated_memory_bytes'])
        self.assertTrue(all(item['driver_version'] is None for item in result))
        self.assertNotIn('PRIVATE_SERIAL', json.dumps(result))

    def test_linux_nvidia_capacity_and_partial_source_failure(self):
        with patch('local_activity_monitor.gpu_info.sys.platform', 'linux'), patch('local_activity_monitor.gpu_info.shutil.which', side_effect=lambda name:'/bin/nvidia-smi' if name == 'nvidia-smi' else None), patch('local_activity_monitor.gpu_info._command', return_value='GPU, 24576, 580.0, 00000000:01:00.0\nOther GPU, N/A, N/A, 00000000:02:00.0'), patch.object(Path, 'iterdir', side_effect=PermissionError()):
            result = gpu_info()
        self.assertEqual(result[0]['dedicated_memory_bytes'], 24*1024**3)
        self.assertEqual(result[0]['driver_version'], '580.0')
        self.assertIsNone(result[1]['dedicated_memory_bytes'])
        self.assertIsNone(result[1]['driver_version'])
        self.assertNotIn('01:00', json.dumps(result))

    def test_linux_drm_capacity_unknown_and_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, value in enumerate(('invalid', '0', str(24*1024**3))):
                device = root/f'card{index}'/'device'
                device.mkdir(parents=True)
                (device/'product_name').write_text(f'AMD GPU {index}', encoding='utf-8')
                (device/'mem_info_vram_total').write_text(value, encoding='utf-8')
                (device/'serial_number').write_text('PRIVATE_SERIAL', encoding='utf-8')
            source_path = Path
            # Emulate Linux sysfs resolution; Windows sandbox denies final-path queries.
            with patch('local_activity_monitor.gpu_info.sys.platform', 'linux'), patch('local_activity_monitor.gpu_info.shutil.which', return_value=None), patch('local_activity_monitor.gpu_info.Path', side_effect=lambda value:root if value == '/sys/class/drm' else source_path(value)), patch.object(Path, 'resolve', lambda path, strict=False:path.absolute()):
                result = gpu_info()
        self.assertEqual(len(result), 3)
        capacities = {item['name']: item['dedicated_memory_bytes'] for item in result}
        self.assertEqual(capacities, {'AMD GPU 0': None, 'AMD GPU 1': 0, 'AMD GPU 2': 24*1024**3})
        self.assertNotIn('PRIVATE_SERIAL', json.dumps(result))

    def test_queries_timeout_and_oversized_or_invalid_reports_are_unknown(self):
        from local_activity_monitor.gpu_info import _command
        for error in (PermissionError(), subprocess.TimeoutExpired('query', 2)):
            with patch('local_activity_monitor.gpu_info.subprocess.run', side_effect=error):
                self.assertEqual(_command(['query']), '')
        with patch('local_activity_monitor.gpu_info.subprocess.run', return_value=Mock(stdout='x'*(128*1024+1))):
            self.assertEqual(_command(['query']), '')
        with patch('local_activity_monitor.gpu_info.sys.platform', 'darwin'), patch('local_activity_monitor.gpu_info._command', return_value='invalid'):
            self.assertEqual(gpu_info(), [])

    def test_32_bit_windows_does_not_report_truncated_capacity(self):
        library, member, released = self.fixture([('GPU', 8*1024**3, 8*1024**3, 0)])
        with patch('local_activity_monitor.gpu_info.sys.platform', 'win32'), patch.object(ctypes, 'WinDLL', return_value=library, create=True), patch('local_activity_monitor.gpu_info._method', side_effect=member), patch('local_activity_monitor.gpu_info._windows_adapter_counts', return_value=None), patch('local_activity_monitor.gpu_info.ctypes.sizeof', return_value=4):
            result = gpu_info()[0]
        self.assertIsNone(result['dedicated_memory_bytes'])
        self.assertIsNone(result['shared_memory_bytes'])
        self.assertEqual(result['driver_version'], '32.1.101.7076')
        self.assertEqual(released, [2, 1])

    def test_physical_device_counts_deduplicate_interfaces_without_merging_two_cards(self):
        for physical, expected in ((1, 1), (2, 2), (None, 3)):
            with self.subTest(physical=physical):
                library, member, released = self.fixture([('Same GPU',8*1024**3,0,0)]*3)
                def identified(pointer,index,result,*arguments):
                    method = member(pointer,index,result,*arguments)
                    if index != 10:
                        return method
                    def describe(adapter,output):
                        status = method(adapter,output)
                        output._obj.vendor,output._obj.device,output._obj.subsystem = 0x10de,0x2484,123
                        output._obj.luid[0] = adapter.value
                        return status
                    return describe
                counts = {(0x10de,0x2484,123):physical} if physical is not None else None
                with patch('local_activity_monitor.gpu_info.sys.platform','win32'),patch.object(ctypes,'WinDLL',return_value=library,create=True),patch('local_activity_monitor.gpu_info._method',side_effect=identified),patch('local_activity_monitor.gpu_info._windows_adapter_counts',return_value=counts):
                    self.assertEqual(len(gpu_info()),expected)
                self.assertEqual(released,[2,3,4,1])
