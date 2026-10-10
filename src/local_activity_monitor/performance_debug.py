"""Opt-in, bounded performance records without source paths or payloads."""
from collections import deque
from datetime import datetime, timezone
from functools import lru_cache
import ctypes
import json
import math
import os
from pathlib import Path
import sys
import threading
import time
import uuid

from .history_store import unsafe


class MemoryCounters(ctypes.Structure):
    _fields_ = [("cb", ctypes.c_uint32), ("faults", ctypes.c_uint32)]+[(name, ctypes.c_size_t) for name in
        ("peak", "working", "peak_paged", "paged", "peak_nonpaged", "nonpaged", "pagefile", "peak_pagefile")]


@lru_cache(maxsize=1)
def windows_memory_api():
    kernel = ctypes.WinDLL("kernel32.dll", winmode=0x800)
    kernel.GetCurrentProcess.argtypes, kernel.GetCurrentProcess.restype = [], ctypes.c_void_p
    psapi = ctypes.WinDLL("psapi.dll", winmode=0x800)
    psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.POINTER(MemoryCounters), ctypes.c_uint32]
    psapi.GetProcessMemoryInfo.restype = ctypes.c_int
    return psapi.GetProcessMemoryInfo, kernel.GetCurrentProcess()


def process_memory():
    result = {"rss_bytes": None, "peak_rss_bytes": None}
    try:
        if sys.platform == "win32":
            read, process = windows_memory_api()
            counters = MemoryCounters();counters.cb = ctypes.sizeof(counters)
            if read(process, ctypes.byref(counters), counters.cb):
                result.update(rss_bytes=counters.working, peak_rss_bytes=counters.peak)
        elif sys.platform.startswith("linux"):
            result["rss_bytes"] = int(Path("/proc/self/statm").read_text(encoding="ascii")[:256].split()[1])*os.sysconf("SC_PAGE_SIZE")
        if sys.platform != "win32":
            import resource
            result["peak_rss_bytes"] = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss*(1 if sys.platform == "darwin" else 1024)
    except (OSError, ValueError, AttributeError, IndexError, ImportError):
        pass
    return result


class PerformanceDebug:
    FILE_BYTES, FILE_COUNT = 8*1024*1024, 4
    RECORD_LIMIT, READ_BYTES = 150, 256*1024
    KINDS = {"enabled", "disabled", "collection", "stream_open", "stream_frame", "stream_close", "stream_error", "heartbeat"}
    FIELDS = {"refresh_ms", "cpu_ms", "read_bytes", "retained_calls", "buffer_bytes", "trimmed_calls", "sql_records",
              "web_records", "mcp_records", "snapshot_bytes", "transfer_bytes", "serialization_ms", "write_ms", "active_streams"}
    PHASES = {"sessions", "projections", "worktrees", "account", "diagnostics", "history", "mcp", "jev", "hardware"}

    def __init__(self, path=None):
        self.path = path
        self.lock = threading.RLock()
        self.enabled = False
        self.health, self.error_type = "disabled", None
        self.records = deque(maxlen=self.RECORD_LIMIT)
        self.session = uuid.uuid4().hex
        self.began = time.monotonic()
        self.revision = None
        self.active_streams = 0
        self.loaded = self.invalid = False
        self.files = []

    def paths(self):
        return [self.path]+[self.path.with_name(self.path.name+f".{index}") for index in range(1, self.FILE_COUNT)] if self.path else []

    def check_paths(self):
        if any(unsafe(path) for path in self.paths()) or self.path and unsafe(self.path.parent):
            raise OSError("Unsafe performance journal")

    def load(self):
        if self.loaded:
            return
        self.loaded = True
        try:
            self.check_paths()
            for path in reversed(self.paths()):
                if not path.exists():
                    continue
                with path.open("rb") as stream:
                    first = json.loads(stream.readline(8192))
                    if first.get("application") != "local-activity-monitor" or first.get("version") != 1 or first.get("kind") not in self.KINDS:
                        raise ValueError("Invalid performance journal")
                    size = path.stat().st_size
                    stream.seek(max(0, size-self.READ_BYTES))
                    lines = stream.read(self.READ_BYTES).splitlines()
                    if size > self.READ_BYTES:
                        lines = lines[1:]
                for raw in lines[-self.RECORD_LIMIT:]:
                    try:
                        item = json.loads(raw)
                        if item.get("application") == "local-activity-monitor" and item.get("version") == 1 and item.get("kind") in self.KINDS:
                            self.records.append(self.project(item))
                    except (ValueError, TypeError, AttributeError):
                        continue
            self.update_files()
        except (OSError, ValueError, TypeError, AttributeError) as error:
            self.invalid = not isinstance(error, OSError)
            self.health, self.error_type = "unavailable", type(error).__name__

    @staticmethod
    def numeric(value):
        return type(value) in (int, float) and 0 <= value <= 1e18 and math.isfinite(value)

    def project(self, value):
        result = {key: item for key, item in value.items() if key in self.FIELDS|{"uptime_ms", "process_cpu_ms", "rss_bytes", "peak_rss_bytes"} and self.numeric(item)}
        for key, maximum in (("timestamp", 40), ("session", 32), ("revision", 40)):
            item = value.get(key)
            if isinstance(item, str) and len(item) <= maximum and all(char in "0123456789abcdef-:.TZ+" for char in item):
                result[key] = item
        result["kind"] = value["kind"]
        phases = value.get("phase_metrics", {})
        if isinstance(phases, dict):
            result["phase_metrics"] = {phase: {key: item for key, item in metrics.items() if key in ("cpu_ms", "elapsed_ms") and self.numeric(item)}
                for phase, metrics in phases.items() if phase in self.PHASES and isinstance(metrics, dict)}
        return result

    def update_files(self):
        self.files = [{"name": path.name, "bytes": path.stat().st_size} for path in self.paths() if path.exists()]

    def set_enabled(self, enabled):
        with self.lock:
            self.load()
            if enabled == self.enabled:
                return
            if enabled:
                self.enabled = True
                self.record("enabled")
            else:
                self.record("disabled")
                self.enabled = False

    def connection(self, opened):
        with self.lock:
            self.active_streams += 1 if opened else -1
            self.record("stream_open" if opened else "stream_close", {"active_streams": self.active_streams})

    def record(self, kind, metrics=None):
        if not self.enabled or kind not in self.KINDS:
            return
        with self.lock:
            if not self.enabled:
                return
            record = self.project({**(metrics or {}), "kind": kind, "timestamp": datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"),
                "session": self.session, "revision": self.revision, "uptime_ms": round((time.monotonic()-self.began)*1000, 2),
                "process_cpu_ms": round(time.process_time()*1000, 2), **process_memory()})
            self.records.append(record)
            if self.path is None:
                self.health = "memory"
                return
            try:
                self.check_paths()
                if self.invalid:
                    return
                self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                raw = (json.dumps({"application": "local-activity-monitor", "version": 1, **record}, separators=(",", ":"))+"\n").encode()
                paths = self.paths()
                if self.path.exists() and self.path.stat().st_size+len(raw) > self.FILE_BYTES:
                    for index in range(len(paths)-1, 0, -1):
                        if paths[index-1].exists():
                            os.replace(paths[index-1], paths[index])
                with self.path.open("ab") as stream:
                    stream.write(raw)
                if os.name != "nt":
                    self.path.chmod(0o600)
                self.update_files()
                self.health, self.error_type = "ok", None
            except OSError as error:
                self.health, self.error_type = "unavailable", type(error).__name__

    def snapshot(self):
        with self.lock:
            self.load()
            return {"enabled": self.enabled, "health": self.health, "error_type": self.error_type, "active_streams": self.active_streams,
                "byte_limit": self.FILE_BYTES*self.FILE_COUNT, "bytes": sum(file["bytes"] for file in self.files), "files": list(self.files),
                "path": str(self.path) if self.path is not None else None,
                "record_limit": self.RECORD_LIMIT, "records": [dict(record) for record in reversed(self.records)]}
