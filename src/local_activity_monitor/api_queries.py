"""One service-owned schedule for explicit, read-only API sources."""

import copy
import threading
import time


class ApiQueries:
    DEFAULT_INTERVAL = 300
    MIN_INTERVAL = 60
    MAX_INTERVAL = 86400

    def __init__(self, sources, notify):
        self.sources, self.notify = sources, notify
        self.intervals = {key: self.DEFAULT_INTERVAL for key in sources}
        self.enabled = {key: False for key in sources}
        self.results, self.completed, self.due = {}, {}, {}
        self.condition = threading.Condition()
        self.stopped = False
        self.thread = threading.Thread(target=self._poll, name="api-queries", daemon=True)

    def configure(self, enabled, intervals):
        with self.condition:
            stamp = time.monotonic()
            for key in self.sources:
                active = enabled[key]
                if active and not self.enabled[key]:
                    self.due[key] = stamp
                elif active and intervals[key] != self.intervals[key]:
                    self.due[key] = self.completed.get(key, stamp)+intervals[key]
                elif not active:
                    self.due.pop(key, None)
                self.enabled[key], self.intervals[key] = active, intervals[key]
            self.condition.notify_all()

    def query(self, key, cursor=None):
        # Source-level locks and 60-second caches coalesce concurrent clients.
        value = self.sources[key](cursor)
        if cursor is None:
            with self.condition:
                self.results[key] = copy.deepcopy(value)
                self.completed[key] = time.monotonic()
                if self.enabled[key]:
                    self.due[key] = self.completed[key]+self.intervals[key]
                self.condition.notify_all()
            self.notify()
        return value

    def snapshot(self):
        with self.condition:
            return copy.deepcopy(self.results)

    def _poll(self):
        while True:
            with self.condition:
                if self.stopped:
                    return
                stamp = time.monotonic()
                ready = next((key for key, due in self.due.items() if due <= stamp), None)
                if ready is None:
                    self.condition.wait(max(0, min(self.due.values())-stamp) if self.due else None)
                    continue
                # A source failure still advances its deadline, without a tight retry loop.
                self.due[ready] = stamp+self.intervals[ready]
            try:
                self.query(ready)
            except Exception:
                with self.condition:
                    self.results[ready] = {"health": "error"}
                self.notify()

    def close(self):
        with self.condition:
            self.stopped = True
            self.condition.notify_all()
        if self.thread.is_alive():
            self.thread.join(timeout=12)
