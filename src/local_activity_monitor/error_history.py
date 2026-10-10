"""A small 24-hour cache of error metadata, never original log messages."""
from datetime import datetime, timedelta, timezone
import hashlib
import json
import sqlite3
import threading

from .error_records import identifier
from .history_store import HistoryStore


def error_identity(event):
    timestamp = event.get("timestamp")
    try:
        timestamp = datetime.fromisoformat(timestamp.replace("Z", "+00:00")).astimezone(timezone.utc).isoformat(timespec="milliseconds")
    except (ValueError, TypeError, AttributeError):
        pass
    return (timestamp,)+tuple(event.get(key) for key in ("source", "category", "severity", "code", "module", "thread_id", "call_id", "attempt", "index", "record_id"))


class ErrorHistory:
    LIMIT = 1000
    BYTE_LIMIT = 512*1024

    def __init__(self, path):
        self.store = HistoryStore(path, "errors")
        self.path = self.store.path
        self.lock = threading.Lock()
        self.events = []
        self.saved = None
        self.last_incoming = None
        self.health, self.error_type = "waiting", None
        try:
            raw = self.store.load(self.BYTE_LIMIT)
            if raw is not None:
                if len(raw) > self.BYTE_LIMIT:
                    raise ValueError()
                value = json.loads(raw)
                if not isinstance(value, dict) or value.get("version") != 1 or not isinstance(value.get("events"), list):
                    raise ValueError()
                self.events = self.project(value["events"][:self.LIMIT])
                if self.store.imported:
                    self.store.save(json.dumps({"version": 1, "events": self.events}, ensure_ascii=True, separators=(",", ":")).encode())
            self.health = "ok"
        except (OSError, ValueError, sqlite3.Error) as error:
            self.health, self.error_type = "unavailable", type(error).__name__

    def project(self, events, bounded=True):
        cutoff = datetime.now(timezone.utc)-timedelta(hours=24) if bounded else datetime.min.replace(tzinfo=timezone.utc)
        ceiling = datetime.now(timezone.utc)+timedelta(minutes=1)
        result = {}
        for value in events:
            if not isinstance(value, dict) or value.get("severity") not in ("error", "warning"):
                continue
            try:
                date = datetime.fromisoformat(value["timestamp"].replace("Z", "+00:00"))
                if not date.tzinfo or not cutoff <= date <= ceiling:
                    continue
            except (KeyError, ValueError, TypeError, AttributeError):
                continue
            event = {"timestamp": date.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")}
            for key in ("source", "category", "severity", "code", "reason", "cause", "error_type", "module", "method", "server", "tool", "call_id", "thread_id", "file", "request_id", "trace_id", "record_hash", "content_id"):
                item = identifier(value.get(key))
                if item is not None:
                    event[key] = item
            if not {"source", "category", "severity"} <= event.keys():
                continue
            for key in ("exit_code", "http_status", "attempt", "index", "record_id", "record_offset"):
                item = value.get(key)
                if type(item) is int and -2**31 <= item <= 2**31-1:
                    event[key] = item
            # Stable identity excludes enrichment fields, so late metadata does not duplicate errors
            identity = error_identity(event)
            result[identity] = event
        ordered = sorted(result.values(), key=lambda event:event["timestamp"], reverse=True)
        return ordered[:self.LIMIT] if bounded else ordered

    def update(self, events):
        with self.lock:
            storage_error = None
            incoming = json.dumps({'version': 1, 'events': self.project(events, False)}, ensure_ascii=True, separators=(',', ':')).encode()
            digest = hashlib.sha256(incoming).digest()
            try:
                if digest != self.last_incoming or self.health != 'ok':
                    self.store.save(incoming)
            except (OSError, sqlite3.Error) as error:
                self.health, self.error_type = 'unavailable', type(error).__name__
                storage_error = error
            self.events = self.project(self.events+events)
            raw = json.dumps({"version": 1, "events": self.events}, ensure_ascii=True, separators=(",", ":")).encode("utf-8")
            while len(raw) > self.BYTE_LIMIT and self.events:
                self.events.pop()
                raw = json.dumps({"version": 1, "events": self.events}, ensure_ascii=True, separators=(",", ":")).encode("utf-8")
            if storage_error is not None:
                return
            try:
                if raw != self.saved and raw != incoming:
                    self.store.save(raw)
                self.saved = raw
                self.last_incoming = digest
                self.health, self.error_type = "ok", None
            except (OSError, sqlite3.Error) as error:
                self.health, self.error_type = "unavailable", type(error).__name__

    def snapshot(self):
        with self.lock:
            return self.project(self.events)
