"""Read available GPU metadata once, with platform-specific capability fallbacks."""
import csv
from collections import Counter
import ctypes
from itertools import islice
import json
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import time
from uuid import UUID


class AdapterDescription(ctypes.Structure):
    _fields_ = [("name", ctypes.c_wchar*128), ("vendor", ctypes.c_uint32),
                ("device", ctypes.c_uint32), ("subsystem", ctypes.c_uint32),
                ("revision", ctypes.c_uint32), ("dedicated", ctypes.c_size_t),
                ("system", ctypes.c_size_t), ("shared", ctypes.c_size_t),
                ("luid", ctypes.c_uint32*2), ("flags", ctypes.c_uint32)]


def _method(pointer, index, result, *arguments):
    """Bind the documented DXGI COM vtable member with its native signature."""
    table = ctypes.cast(pointer, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
    return ctypes.WINFUNCTYPE(result, ctypes.c_void_p, *arguments)(table[index])


def _windows_adapter_counts():
    """Count present PCI display devices, excluding virtual display interfaces."""
    guid_type = ctypes.c_ubyte*16
    class DeviceInfo(ctypes.Structure):
        _fields_ = [('size',ctypes.c_uint32),('guid',guid_type),('instance',ctypes.c_uint32),('reserved',ctypes.c_void_p)]
    handle = None
    try:
        setup = ctypes.WinDLL('setupapi.dll', winmode=0x800)
        setup.SetupDiGetClassDevsW.argtypes = [ctypes.POINTER(guid_type),ctypes.c_wchar_p,ctypes.c_void_p,ctypes.c_uint32]
        setup.SetupDiGetClassDevsW.restype = ctypes.c_void_p
        setup.SetupDiEnumDeviceInfo.argtypes = [ctypes.c_void_p,ctypes.c_uint32,ctypes.POINTER(DeviceInfo)]
        setup.SetupDiEnumDeviceInfo.restype = ctypes.c_int
        setup.SetupDiGetDeviceInstanceIdW.argtypes = [ctypes.c_void_p,ctypes.POINTER(DeviceInfo),ctypes.c_wchar_p,ctypes.c_uint32,ctypes.POINTER(ctypes.c_uint32)]
        setup.SetupDiGetDeviceInstanceIdW.restype = ctypes.c_int
        setup.SetupDiDestroyDeviceInfoList.argtypes = [ctypes.c_void_p]
        setup.SetupDiDestroyDeviceInfoList.restype = ctypes.c_int
        guid = guid_type.from_buffer_copy(UUID('4d36e968-e325-11ce-bfc1-08002be10318').bytes_le)
        handle = setup.SetupDiGetClassDevsW(ctypes.byref(guid),None,None,2)
        if handle in (None,ctypes.c_void_p(-1).value):
            handle = None
            return None
        counts = Counter()
        for index in range(64):
            info = DeviceInfo();info.size = ctypes.sizeof(info)
            if not setup.SetupDiEnumDeviceInfo(handle,index,ctypes.byref(info)):
                break
            identity = ctypes.create_unicode_buffer(512)
            if setup.SetupDiGetDeviceInstanceIdW(handle,ctypes.byref(info),identity,512,None):
                match = re.match(r'PCI\\VEN_([0-9A-F]{4})&DEV_([0-9A-F]{4})&SUBSYS_([0-9A-F]{8})',identity.value,re.I)
                if match:
                    counts[tuple(int(value,16) for value in match.groups())] += 1
        return counts
    except (OSError,AttributeError,ValueError,TypeError):
        return None
    finally:
        if handle:
            setup.SetupDiDestroyDeviceInfoList(handle)


def _windows_gpus():
    gpus, factory, seen, counts = [], ctypes.c_void_p(), set(), _windows_adapter_counts()
    physical = Counter()
    guid_type = ctypes.c_ubyte*16
    factory_id = guid_type.from_buffer_copy(UUID("770aae78-f26f-4dba-a829-253c83d1b387").bytes_le)
    device_id = guid_type.from_buffer_copy(UUID("54ec77fa-1377-44e6-8c32-88fd5f44c84c").bytes_le)
    try:
        dxgi = ctypes.WinDLL("dxgi.dll", winmode=0x800)  # System32 only.
        create = dxgi.CreateDXGIFactory1
        create.argtypes = [ctypes.POINTER(guid_type), ctypes.POINTER(ctypes.c_void_p)]
        create.restype = ctypes.c_int32
        if create(ctypes.byref(factory_id), ctypes.byref(factory)) != 0 or not factory:
            return []
        enumerate_adapters = _method(factory, 12, ctypes.c_int32, ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p))
        for index in range(16):
            adapter = ctypes.c_void_p()
            if enumerate_adapters(factory, index, ctypes.byref(adapter)) != 0 or not adapter:
                break
            try:
                description, version = AdapterDescription(), ctypes.c_uint64()
                if _method(adapter, 10, ctypes.c_int32, ctypes.POINTER(AdapterDescription))(adapter, ctypes.byref(description)) != 0 or description.flags & 3:
                    continue  # Skip remote and software adapters.
                identity = tuple(description.luid)
                if any(identity) and identity in seen:
                    continue
                seen.add(identity)
                device = (description.vendor,description.device,description.subsystem)
                if counts is not None and counts.get(device):
                    if physical[device] >= counts[device]:
                        continue
                    physical[device] += 1
                driver = None
                if _method(adapter, 9, ctypes.c_int32, ctypes.POINTER(guid_type), ctypes.POINTER(ctypes.c_uint64))(adapter, ctypes.byref(device_id), ctypes.byref(version)) == 0:
                    driver = ".".join(str((version.value >> shift) & 0xffff) for shift in (48, 32, 16, 0))
                gpus.append({"name": description.name.strip() or None,
                             "dedicated_memory_bytes": description.dedicated if ctypes.sizeof(ctypes.c_void_p) == 8 else None,
                             "shared_memory_bytes": description.shared if ctypes.sizeof(ctypes.c_void_p) == 8 else None,
                             "driver_version": driver})
            finally:
                _method(adapter, 2, ctypes.c_uint32)(adapter)
    except (OSError, AttributeError, ValueError):
        pass
    finally:
        if factory:
            _method(factory, 2, ctypes.c_uint32)(factory)
    return gpus


def _command(arguments, timeout=2):
    """Run a fixed local hardware query with a timeout and bounded accepted output."""
    try:
        result = subprocess.run(arguments, capture_output=True, text=True, timeout=timeout, check=True)
        return result.stdout if len(result.stdout) <= 128*1024 else ""
    except (OSError, subprocess.SubprocessError, UnicodeError):
        return ""


def _read(path):
    try:
        with path.open(encoding="utf-8") as stream:
            return stream.read(256).strip()
    except (OSError, UnicodeError):
        return ""


def _capacity(value):
    try:
        number = int(value)
        return number if 0 <= number < 2**63 else None
    except (ValueError, TypeError, OverflowError):
        return None


def _linux_gpus():
    deadline = time.monotonic()+3
    gpus, nvidia_slots = [], set()
    nvidia = shutil.which("nvidia-smi")
    if nvidia:
        output = _command([nvidia, "--query-gpu=name,memory.total,driver_version,pci.bus_id", "--format=csv,noheader,nounits"])
        for row in islice(csv.reader(output.splitlines()), 16):
            if len(row) != 4:
                continue
            name, memory, driver, slot = (value.strip() for value in row)
            capacity = _capacity(memory)
            gpus.append({"name": name[:160] or None, "dedicated_memory_bytes": capacity*1024**2 if capacity is not None else None,
                         "shared_memory_bytes": None, "driver_version": driver if driver not in ("", "N/A", "[N/A]") else None})
            nvidia_slots.add(slot.lower().lstrip("0"))
    lspci = shutil.which("lspci")
    try:
        cards = [path for path in islice(Path("/sys/class/drm").iterdir(), 64) if re.fullmatch(r"card\d+", path.name)][:16]
    except OSError:
        return gpus
    for card in cards:
        try:
            device = (card/"device").resolve(strict=True)
        except OSError:
            continue
        if device.name.lower().lstrip("0") in nvidia_slots:
            continue
        name = _read(device/"product_name") or None
        if not name and lspci and time.monotonic() < deadline and re.fullmatch(r"[\da-fA-F]{4}:[\da-fA-F]{2}:[\da-fA-F]{2}\.[0-7]", device.name):
            try:
                fields = shlex.split(_command([lspci, "-mm", "-s", device.name], timeout=min(2, max(.01, deadline-time.monotonic()))))
                if len(fields) >= 4:
                    name = " ".join(fields[2:4])[:160]
            except ValueError:
                pass
        try:
            driver = (device/"driver").resolve().name
        except OSError:
            driver = ""
        # Module versions are optional. A kernel release is not a GPU driver version.
        version = _read(Path("/sys/module")/driver/"version") if re.fullmatch(r"[\w-]{1,80}", driver) else ""
        gpus.append({"name": name, "dedicated_memory_bytes": _capacity(_read(device/"mem_info_vram_total")),
                     "shared_memory_bytes": None, "driver_version": version or None})
        if len(gpus) >= 16:
            break
    return gpus[:16]


def _mac_gpus():
    try:
        data = json.loads(_command(["/usr/sbin/system_profiler", "-json", "-detailLevel", "mini", "SPDisplaysDataType"]))
    except (ValueError, TypeError):
        return []
    items = data.get("SPDisplaysDataType") if isinstance(data, dict) else None
    if not isinstance(items, list):
        return []
    gpus = []
    for item in items[:16]:
        if not isinstance(item, dict):
            continue
        name = item.get("sppci_model") or item.get("_name")
        if not isinstance(name, str):
            continue
        memory = item.get("spdisplays_vram")
        match = re.fullmatch(r"(\d+(?:\.\d+)?)\s*(MB|GB|MiB|GiB)", memory) if isinstance(memory, str) else None
        capacity = _capacity(float(match[1])*1024**(2 if match[2] in ("MB", "MiB") else 3)) if match else None
        unified = bool(re.match(r"^Apple M\d", name))
        gpus.append({"name": name[:160], "dedicated_memory_bytes": None if unified else capacity,
                     "shared_memory_bytes": None, "driver_version": None,
                     "memory_type": "unified" if unified else None})
    return gpus


def gpu_info():
    try:
        if sys.platform == "win32":
            return _windows_gpus()
        if sys.platform.startswith("linux"):
            return _linux_gpus()
        if sys.platform == "darwin":
            return _mac_gpus()
    except (OSError, ValueError, AttributeError):
        pass
    return []
