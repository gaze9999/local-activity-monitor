"""Bounded process metrics; retain types and counters, never exception text."""
from collections import deque
from datetime import datetime, timezone
from importlib.metadata import PackageNotFoundError, version
import os
import platform
import threading
import time


def stamp():
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


class MonitorState:
    HISTORY_LIMIT = 360
    EVENT_LIMIT = 200

    def __init__(self):
        self.lock = threading.Lock()
        self.began = time.monotonic()
        self.history = deque(maxlen=self.HISTORY_LIMIT)
        self.events = deque(maxlen=self.EVENT_LIMIT)
        self.refreshes = self.errors = self.requests = self.http_errors = 0
        self.health, self.error_type, self.last_error_at = "starting", None, None
        self.runtime = {"python": platform.python_version(), "platform": platform.system(), "architecture": platform.machine(), "pid": os.getpid()}
        try:
            self.runtime["version"] = version("local-activity-monitor")
        except PackageNotFoundError:
            self.runtime["version"] = None
        self.snapshot_bytes = self.transfer_bytes = 0
        self.event("started")

    def event(self, kind, error_type=None):
        with self.lock:
            self.events.append({"timestamp": stamp(), "kind": kind, "error_type": error_type})

    def failed(self, error):
        with self.lock:
            self.errors += 1
            self.last_error_at = stamp()
            category = type(error).__name__[:80]
            # Consecutive failures have one event, with a cumulative counter
            if self.health != "error" or category != self.error_type:
                self.events.append({"timestamp": self.last_error_at, "kind": "refresh_failed", "error_type": category})
            self.health, self.error_type = "error", category

    def refreshed(self, metrics):
        with self.lock:
            if self.health == "error":
                self.events.append({"timestamp": stamp(), "kind": "recovered", "error_type": None})
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
                    self.events.append({"timestamp": stamp(), "kind": "http_response_error", "error_type": None, "http_status": code})
            if snapshot_bytes is not None:
                self.snapshot_bytes, self.transfer_bytes = snapshot_bytes, transfer_bytes

    def snapshot(self):
        with self.lock:
            return {**self.runtime, "health": self.health, "uptime_seconds": round(time.monotonic()-self.began), "refreshes": self.refreshes, "errors": self.errors, "requests": self.requests, "http_errors": self.http_errors, "last_error_at": self.last_error_at, "error_type": self.error_type, **(self.history[-1] if self.history else {}), "snapshot_bytes": self.snapshot_bytes, "transfer_bytes": self.transfer_bytes, "history_limit": self.HISTORY_LIMIT, "event_limit": self.EVENT_LIMIT, "history": [dict(item) for item in self.history], "events": [dict(item) for item in self.events]}
