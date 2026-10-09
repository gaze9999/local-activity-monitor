"""Read device capacity using local OS metadata only."""
import ctypes
import os
from pathlib import Path
import platform
import subprocess
import sys
import re
import shutil


def _native_fonts():
    families = set()
    if sys.platform == 'win32':
        class Font(ctypes.Structure):
            _fields_ = [(name,ctypes.c_int32) for name in ('height','width','escape','orientation','weight')]+[(name,ctypes.c_ubyte) for name in ('italic','underline','strike','charset','precision','clip','quality','pitch')]+[('face',ctypes.c_wchar*32)]
        gdi = ctypes.WinDLL('gdi32.dll',winmode=0x800)
        gdi.CreateCompatibleDC.argtypes = [ctypes.c_void_p];gdi.CreateCompatibleDC.restype = ctypes.c_void_p
        gdi.DeleteDC.argtypes = [ctypes.c_void_p];gdi.DeleteDC.restype = ctypes.c_int
        callback_type = ctypes.WINFUNCTYPE(ctypes.c_int,ctypes.POINTER(Font),ctypes.c_void_p,ctypes.c_uint32,ctypes.c_ssize_t)
        def collect(font,metrics,kind,context):
            families.add(font.contents.face.lstrip('@'))
            return int(len(families)<2000)
        callback = callback_type(collect)
        gdi.EnumFontFamiliesExW.argtypes = [ctypes.c_void_p,ctypes.POINTER(Font),callback_type,ctypes.c_ssize_t,ctypes.c_uint32]
        gdi.EnumFontFamiliesExW.restype = ctypes.c_int
        dc = gdi.CreateCompatibleDC(None)
        if dc:
            try:
                font = Font();font.charset = 1
                gdi.EnumFontFamiliesExW(dc,ctypes.byref(font),callback,0,0)
            finally:
                gdi.DeleteDC(dc)
    elif sys.platform == 'darwin':
        core = ctypes.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
        text = ctypes.CDLL('/System/Library/Frameworks/CoreText.framework/CoreText')
        text.CTFontManagerCopyAvailableFontFamilyNames.argtypes = [];text.CTFontManagerCopyAvailableFontFamilyNames.restype = ctypes.c_void_p
        core.CFArrayGetCount.argtypes = [ctypes.c_void_p];core.CFArrayGetCount.restype = ctypes.c_ssize_t
        core.CFArrayGetValueAtIndex.argtypes = [ctypes.c_void_p,ctypes.c_ssize_t];core.CFArrayGetValueAtIndex.restype = ctypes.c_void_p
        core.CFStringGetCString.argtypes = [ctypes.c_void_p,ctypes.c_char_p,ctypes.c_ssize_t,ctypes.c_uint32];core.CFStringGetCString.restype = ctypes.c_bool
        core.CFRelease.argtypes = [ctypes.c_void_p];core.CFRelease.restype = None
        names = text.CTFontManagerCopyAvailableFontFamilyNames()
        if names:
            try:
                for index in range(min(core.CFArrayGetCount(names),2000)):
                    buffer = ctypes.create_string_buffer(1024)
                    if core.CFStringGetCString(core.CFArrayGetValueAtIndex(names,index),buffer,1024,0x08000100):
                        families.add(buffer.value.decode('utf-8'))
            finally:
                core.CFRelease(names)
    return families


def system_fonts():
    """Return bounded local family names, never font file locations."""
    families = set()
    try:
        if sys.platform in ('win32','darwin'):
            families.update(_native_fonts())
        if sys.platform == 'win32' and not families:
            import winreg
            for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
                try:
                    with winreg.OpenKey(hive, r'SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts') as key:
                        for index in range(min(winreg.QueryInfoKey(key)[1], 2000)):
                            label = winreg.EnumValue(key, index)[0]
                            label = re.sub(r'\s*\([^()]*\)\s*$', '', label)
                            families.update(value.strip() for value in label.split(' & '))
                except OSError:
                    continue
        elif sys.platform.startswith('linux') and shutil.which('fc-list'):
            result = subprocess.run(['fc-list', '--format', '%{family}\n'], capture_output=True, timeout=3, text=True, encoding='utf-8', errors='replace')
            if result.returncode == 0:
                families.update(value.strip() for line in result.stdout[:262144].splitlines() for value in line.split(','))
    except (OSError, AttributeError, ValueError, subprocess.TimeoutExpired):
        pass
    return sorted(value for value in families if value and len(value) <= 160 and re.fullmatch(r"[\w\s'._-]+", value))[:500]


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
