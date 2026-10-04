"""Bounded process metrics; retain types and counters, never exception text."""
from collections import deque
from datetime import datetime, timezone
from . import __version__
import os
import platform
import json
import re
import threading
import time


def stamp():
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


class MonitorState:
    HISTORY_LIMIT = 360
    EVENT_LIMIT = 200
    LOG_LIMIT = 1000
    JOURNAL_LIMIT = 64*1024

    def __init__(self, journal=None):
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
        self.refreshes = self.errors = self.requests = self.http_errors = 0
        self.health, self.error_type, self.last_error_at = "starting", None, None
        self.runtime = {"python": platform.python_version(), "platform": platform.system(), "architecture": platform.machine(), "pid": os.getpid()}
        self.runtime["version"] = __version__
        self.snapshot_bytes = self.transfer_bytes = 0
        self.load_journal()
        self.event("started")

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
            self.history.append({"time": stamp(), **metrics})

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
                result["entries"] = [dict(event, source="monitor", severity="error" if event["kind"] in ("refresh_failed", "http_response_error") else "info", code=event["kind"], module="local_activity_monitor", file=self.journal.name if self.journal else None) for event in self.logs] if self.log_enabled else []
            return result

    def snapshot(self):
        with self.lock:
            return {**self.runtime, "health": self.health, "uptime_seconds": round(time.monotonic()-self.began), "refreshes": self.refreshes, "errors": self.errors, "requests": self.requests, "http_errors": self.http_errors, "last_error_at": self.last_error_at, "error_type": self.error_type, **(self.history[-1] if self.history else {}), "snapshot_bytes": self.snapshot_bytes, "transfer_bytes": self.transfer_bytes, "history_limit": self.HISTORY_LIMIT, "event_limit": self.EVENT_LIMIT, "history": [dict(item) for item in self.history], "events": [dict(item) for item in self.events]}
