"""Bounded process metrics; retain types and counters, never exception text."""
from collections import deque
from datetime import datetime, timezone
from . import __version__
from .device_info import memory_info, processor_name, CpuUsage, system_fonts
from .gpu_info import gpu_info
import os
import platform
import json
import re
import struct
import threading
import time


def stamp():
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def frontend_report(value):
    """Accept code locations only, never messages, URLs or arbitrary log content."""
    if not isinstance(value, dict) or set(value) != {"phase", "error_type", "frames", "frontend_revision"}:
        raise ValueError("Invalid frontend diagnostic")
    if value["phase"] not in ("render", "script", "promise") or not isinstance(value["error_type"], str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,79}", value["error_type"]):
        raise ValueError("Invalid frontend diagnostic")
    revision = value["frontend_revision"]
    if revision is not None and (not isinstance(revision, str) or not re.fullmatch(r"[a-f0-9]{12}", revision)):
        raise ValueError("Invalid frontend revision")
    frames = value["frames"]
    if not isinstance(frames, list) or len(frames) > 8:
        raise ValueError("Invalid frontend frames")
    for frame in frames:
        if not isinstance(frame, dict) or set(frame) != {"file", "function", "line", "column"} or frame["file"] not in ("index.html", "app.js", "workbench-ui.js"):
            raise ValueError("Invalid frontend frame")
        if not isinstance(frame["function"], str) or not re.fullmatch(r"[A-Za-z0-9_.$<>]{1,80}", frame["function"]) or any(type(frame[key]) is not int or not 0 <= frame[key] <= 10_000_000 for key in ("line", "column")):
            raise ValueError("Invalid frontend frame")
    return {"phase": value["phase"], "frontend_revision": revision, "frames": [dict(frame) for frame in frames]}


class MonitorState:
    HISTORY_LIMIT = 360
    EVENT_LIMIT = 200
    LOG_LIMIT = 1000
    JOURNAL_LIMIT = 64*1024

    def __init__(self, journal=None, defer_device=False):
        self.lock = threading.Lock()
        self.began = time.monotonic()
        self.history = deque(maxlen=self.HISTORY_LIMIT)
        self.events = deque(maxlen=self.EVENT_LIMIT)
        self.logs = deque(maxlen=self.LOG_LIMIT)
        self.journal = journal
        self.log_enabled = True
        self.log_health = "memory" if journal is None else "waiting"
        self.log_error_type = None
        self.log_checked_at = None
        self.log_skipped = self.log_bytes = 0
        self.client_reports = deque(maxlen=20)
        self.last_heartbeat = time.monotonic()
        self.refreshes = self.errors = self.requests = self.http_errors = 0
        self.health, self.error_type, self.last_error_at = "starting", None, None
        cpu_count = getattr(os, "process_cpu_count", os.cpu_count)()
        if cpu_count is None:
            cpu_count = os.cpu_count()
        self.runtime = {"python": platform.python_version(), "platform": platform.system(), "architecture": platform.machine(), "pid": os.getpid(),
                        "system_release": platform.release(), "system_version": platform.version(),
                        "python_implementation": platform.python_implementation(), "process_bits": struct.calcsize("P")*8,
                        "logical_cpus": cpu_count, "processor": None, "gpus": []}
        self.device_ready = False
        self.collection = {"phase": "waiting", "phase_ms": 0, "phase_cpu_ms": 0}
        self.collection_started = (time.perf_counter(), time.process_time())
        self.memory = {}
        self.cpu = CpuUsage()
        self.cpu_percent = None
        self.device_checked_at = stamp()
        self.memory_checked = self.began
        self.runtime["version"] = __version__
        self.snapshot_bytes = self.transfer_bytes = 0
        self.load_journal()
        self.event("started")
        if not defer_device:
            self.load_device()

    def load_device(self):
        if self.device_ready:
            return
        # Hardware queries run in the collector worker, outside the snapshot lock.
        processor, gpus, memory, fonts = processor_name(), gpu_info(), memory_info(), system_fonts()
        with self.lock:
            self.runtime.update(processor=processor, gpus=gpus, fonts=fonts)
            self.memory = memory
            self.device_ready = True
            self.device_checked_at = stamp()
            self.memory_checked = time.monotonic()

    def collecting(self, phase, phase_ms=0, phase_cpu_ms=0):
        with self.lock:
            self.collection = {"phase": phase, "phase_ms": phase_ms, "phase_cpu_ms": phase_cpu_ms}
            self.collection_started = (time.perf_counter(), time.process_time())

    def load_journal(self):
        if self.journal is None:
            return
        try:
            for path in (self.journal.with_name(self.journal.name+".1"), self.journal):
                if not path.exists():
                    continue
                if path.is_symlink():
                    raise OSError()
                with path.open("rb") as stream:
                    size = path.stat().st_size
                    stream.seek(max(0, size-self.JOURNAL_LIMIT))
                    lines = stream.read(self.JOURNAL_LIMIT).splitlines()
                    if size > self.JOURNAL_LIMIT and lines:
                        lines.pop(0)
                self.log_bytes += size
                for line in lines:
                    try:
                        value = json.loads(line)
                        if not isinstance(value, dict) or value.get("application") != "local-activity-monitor" or value.get("version") != 1:
                            raise ValueError()
                        event = {key: value.get(key) for key in ("timestamp", "kind", "error_type")}
                        if not isinstance(event["timestamp"], str) or not datetime.fromisoformat(event["timestamp"].replace("Z", "+00:00")).tzinfo or not isinstance(event["kind"], str) or not re.fullmatch(r"[a-z_]{1,80}", event["kind"]) or event["error_type"] is not None and (not isinstance(event["error_type"], str) or not re.fullmatch(r"[a-zA-Z0-9_]{1,80}", event["error_type"])):
                            raise ValueError()
                        if type(value.get("http_status")) is int and 100 <= value["http_status"] <= 599:
                            event["http_status"] = value["http_status"]
                        if event["kind"] == "frontend_error":
                            event.update(frontend_report({key: value.get(key) for key in ("phase", "error_type", "frames", "frontend_revision")}))
                        self.logs.append(event)
                    except (ValueError, TypeError):
                        self.log_skipped += 1
            self.log_health = "ok"
        except OSError as error:
            self.log_health, self.log_error_type = "unavailable", type(error).__name__
        self.log_checked_at = stamp()

    def append_event(self, event):
        """Called under the state lock; disk failures never interrupt collection."""
        self.events.append(event)
        self.logs.append(event)
        if self.journal is None or not self.log_enabled:
            return
        try:
            self.journal.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            previous = self.journal.with_name(self.journal.name+".1")
            if self.journal.is_symlink() or previous.is_symlink():
                raise OSError()
            raw = (json.dumps({"application": "local-activity-monitor", "version": 1, **event}, ensure_ascii=True)+"\n").encode("utf-8")
            size = self.journal.stat().st_size if self.journal.exists() else 0
            if size+len(raw) > self.JOURNAL_LIMIT:
                os.replace(self.journal, previous)
                size = 0
            with self.journal.open("ab") as stream:
                stream.write(raw)
            if os.name != "nt":
                self.journal.chmod(0o600)
            self.log_bytes = size+len(raw)+(previous.stat().st_size if previous.exists() else 0)
            self.log_health, self.log_error_type = "ok", None
        except OSError as error:
            self.log_health, self.log_error_type = "unavailable", type(error).__name__
        self.log_checked_at = stamp()

    def event(self, kind, error_type=None):
        with self.lock:
            self.append_event({"timestamp": stamp(), "kind": kind, "error_type": error_type})

    def frontend_failed(self, value):
        report = frontend_report(value)
        signature = json.dumps(value, sort_keys=True)
        with self.lock:
            current = time.monotonic()
            while self.client_reports and current - self.client_reports[0][0] >= 60:
                self.client_reports.popleft()
            if len(self.client_reports) >= 20 or any(key == signature for _, key in self.client_reports):
                return False
            self.client_reports.append((current, signature))
            self.append_event({"timestamp": stamp(), "kind": "frontend_error", "error_type": value["error_type"], **report})
            return True

    def failed(self, error):
        with self.lock:
            self.errors += 1
            self.last_error_at = stamp()
            category = type(error).__name__[:80]
            # Consecutive failures have one event, with a cumulative counter
            if self.health != "error" or category != self.error_type:
                self.append_event({"timestamp": self.last_error_at, "kind": "refresh_failed", "error_type": category})
            self.health, self.error_type = "error", category

    def refreshed(self, metrics):
        with self.lock:
            if self.health == "error":
                self.append_event({"timestamp": stamp(), "kind": "recovered", "error_type": None})
            self.health, self.error_type = "ok", None
            self.refreshes += 1
            if time.monotonic() - self.last_heartbeat >= 60:
                self.append_event({"timestamp": stamp(), "kind": "heartbeat", "error_type": None})
                self.last_heartbeat = time.monotonic()
            self.history.append({"time": stamp(), "snapshot_bytes": self.snapshot_bytes, "transfer_bytes": self.transfer_bytes, **metrics})

    def requested(self, code, snapshot_bytes=None, transfer_bytes=None):
        with self.lock:
            self.requests += 1
            self.http_errors += code >= 400
            if code >= 400:
                latest = self.events[-1] if self.events else {}
                if latest.get("kind") != "http_response_error" or latest.get("http_status") != code:
                    self.append_event({"timestamp": stamp(), "kind": "http_response_error", "error_type": None, "http_status": code})
            if snapshot_bytes is not None:
                self.snapshot_bytes, self.transfer_bytes = snapshot_bytes, transfer_bytes

    def log_snapshot(self, include_entries=True):
        with self.lock:
            result = {"source": "monitor", "health": self.log_health if self.log_enabled else "disabled", "checked_at": self.log_checked_at, "error_type": self.log_error_type, "files": [{"name": self.journal.name}] if self.journal else [], "bytes": self.log_bytes, "byte_limit": self.JOURNAL_LIMIT*2, "retained": len(self.logs), "unsupported_lines": self.log_skipped}
            if include_entries:
                result["entries"] = [dict(event, source="monitor", severity="error" if event["kind"] in ("refresh_failed", "http_response_error", "frontend_error") else "info", code=event["kind"], module="local_activity_monitor", file=self.journal.name if self.journal else None) for event in self.logs] if self.log_enabled else []
            return result

    def snapshot(self):
        with self.lock:
            collection = dict(self.collection)
            if collection["phase"] not in ("idle", "waiting", "stopped", "error"):
                wall, cpu = self.collection_started
                collection.update(phase_ms=round((time.perf_counter()-wall)*1000, 2), phase_cpu_ms=round((time.process_time()-cpu)*1000, 2))
            if self.device_ready and time.monotonic()-self.memory_checked >= 5:
                if self.runtime["platform"] != "Darwin":
                    self.memory = memory_info()
                self.cpu_percent = self.cpu.sample()
                self.device_checked_at = stamp()
                self.memory_checked = time.monotonic()
            return {**self.runtime, **self.memory, "collection": collection, "device_ready": self.device_ready, "cpu_usage_percent": self.cpu_percent, "cpu_scope": self.cpu.scope, "device_checked_at": self.device_checked_at if self.device_ready else None, "health": self.health, "uptime_seconds": round(time.monotonic()-self.began), "refreshes": self.refreshes, "errors": self.errors, "requests": self.requests, "http_errors": self.http_errors, "last_error_at": self.last_error_at, "error_type": self.error_type, **(self.history[-1] if self.history else {}), "snapshot_bytes": self.snapshot_bytes, "transfer_bytes": self.transfer_bytes, "history_limit": self.HISTORY_LIMIT, "event_limit": self.EVENT_LIMIT, "history": [dict(item) for item in self.history], "events": [dict(item) for item in self.events]}
