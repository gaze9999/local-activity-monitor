"""Read device capacity using local OS metadata only."""
import ctypes
import os
from pathlib import Path
import platform
import subprocess
import sys


def cpu_times():
    """Return aggregate idle and total counters for the available OS scope."""
    try:
        if sys.platform=='win32':
            idle, kernel, user = ctypes.c_uint64(), ctypes.c_uint64(), ctypes.c_uint64()
            function = ctypes.windll.kernel32.GetSystemTimes
            function.argtypes = [ctypes.POINTER(ctypes.c_uint64)]*3
            function.restype = ctypes.c_int
            if function(ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)):
                return idle.value, kernel.value+user.value
        elif sys.platform.startswith('linux'):
            with Path('/proc/stat').open(encoding='ascii') as stream:
                fields = stream.readline(1024).split()
            if fields[0]=='cpu' and len(fields)>=5:
                values = [int(value) for value in fields[1:9]]
                return values[3]+(values[4] if len(values)>4 else 0), sum(values)
        elif sys.platform=='darwin':
            library = ctypes.CDLL('/usr/lib/libSystem.B.dylib')
            library.mach_host_self.argtypes = []
            library.mach_host_self.restype = ctypes.c_uint
            library.host_statistics.argtypes = [ctypes.c_uint, ctypes.c_int, ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_uint)]
            library.host_statistics.restype = ctypes.c_int
            ticks, count = (ctypes.c_uint*4)(), ctypes.c_uint(4)
            host = library.mach_host_self()
            try:
                if library.host_statistics(host, 3, ticks, ctypes.byref(count))==0 and count.value==4:
                    return ticks[2], sum(ticks)
            finally:
                library.mach_port_deallocate.argtypes = [ctypes.c_uint, ctypes.c_uint]
                library.mach_port_deallocate(ctypes.c_uint.in_dll(library, 'mach_task_self_').value, host)
    except (OSError, ValueError, AttributeError, IndexError):
        pass
    return None


class CpuUsage:
    def __init__(self):
        self.previous = cpu_times()
        self.scope = 'processor_group' if sys.platform=='win32' and (os.cpu_count() or 0)>64 else 'system'

    def sample(self):
        current, previous = cpu_times(), self.previous
        self.previous = current
        if not current or not previous:
            return None
        idle, total = current[0]-previous[0], current[1]-previous[1]
        return round((1-idle/total)*100, 1) if total>0 and 0<=idle<=total else None


def processor_name():
    try:
        if sys.platform == "win32":
            import winreg
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") as key:
                return str(winreg.QueryValueEx(key, "ProcessorNameString")[0]).strip()[:160]
        if sys.platform.startswith("linux"):
            with Path("/proc/cpuinfo").open(encoding="utf-8") as stream:
                for line in stream.read(16384).splitlines():
                    if line.startswith("model name"):
                        return line.partition(":")[2].strip()[:160]
        if sys.platform == "darwin":
            result = subprocess.run(["/usr/sbin/sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True, timeout=1, check=True)
            return result.stdout.strip()[:160] or None
    except (OSError, ValueError, subprocess.SubprocessError):
        pass
    return platform.processor()[:160] or None


def memory_info():
    result = {"physical_memory_bytes": None, "available_memory_bytes": None}
    try:
        if sys.platform == "win32":
            class MemoryStatus(ctypes.Structure):
                _fields_ = [("length", ctypes.c_uint32), ("load", ctypes.c_uint32)]+[(name, ctypes.c_uint64) for name in ("total", "available", "page_total", "page_available", "virtual_total", "virtual_available", "extended")]
            status = MemoryStatus()
            status.length = ctypes.sizeof(status)
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
                result.update(physical_memory_bytes=status.total, available_memory_bytes=status.available)
        elif sys.platform.startswith("linux"):
            with Path("/proc/meminfo").open(encoding="ascii") as stream:
                fields = dict(line.split(":", 1) for line in stream.read(16384).splitlines() if ":" in line)
            for source, target in (("MemTotal", "physical_memory_bytes"), ("MemAvailable", "available_memory_bytes")):
                if source in fields:
                    result[target] = int(fields[source].split()[0])*1024
        elif sys.platform == "darwin":
            value = subprocess.run(["/usr/sbin/sysctl", "-n", "hw.memsize"], capture_output=True, text=True, timeout=1, check=True)
            result["physical_memory_bytes"] = int(value.stdout.strip())
    except (OSError, ValueError, AttributeError, subprocess.SubprocessError):
        pass
    return result
