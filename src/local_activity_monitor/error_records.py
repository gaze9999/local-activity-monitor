"""Bounded error metadata from local diagnostics; never retain log bodies."""
from collections import Counter, deque
from contextlib import closing
from datetime import datetime, timezone
import os
from pathlib import Path
import re
import sqlite3
import sys
import time

from .mcp_records import decoded_output

IDENTIFIER = re.compile(r"[a-zA-Z0-9_./:-]{1,160}\Z")
UUID = r"[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}"
FAILURES = {"error", "failed", "timeout", "missing_dependencies", "network_unavailable", "http_error", "invalid_response", "response_too_large"}


def identifier(value):
    return value if isinstance(value, str) and IDENTIFIER.fullmatch(value) else None


def tool_error(output):
    value = decoded_output(output)
    status = value.get("status", value.get("state"))
    status = status.lower() if isinstance(status, str) else None
    exit_code = value.get("exit_code")
    if not value and isinstance(output, str):
        match = re.search(r"^Process exited with code (-?\d{1,6})\s*$", output[:1024*1024], re.M)
        exit_code = int(match[1]) if match else None
    if type(exit_code) is not int or not -2**31 <= exit_code <= 2**31-1:
        exit_code = None
    if value.get("isError") is not True and not value.get("error") and status not in FAILURES and exit_code in (None, 0):
        return None
    error = value.get("error")
    code = identifier(error.get("code")) if isinstance(error, dict) else None
    return {"code": code or identifier(value.get("error_code")) or ("process_exit" if exit_code not in (None, 0) else identifier(status) or "tool_error"), "exit_code": exit_code}


def diagnostic_roots(home):
    """Honor platform directories; isolated fixtures never inspect the real profile."""
    if home.resolve() != (Path(os.environ.get("CODEX_HOME", Path.home()/".codex")).expanduser()).resolve():
        return [home/"desktop-logs"]
    if sys.platform == "win32":
        return [Path(os.environ.get("LOCALAPPDATA", Path.home()/"AppData/Local"))/"Codex/Logs"]
    if sys.platform == "darwin":
        return [Path.home()/"Library/Logs/Codex"]
    return [Path(os.environ.get("XDG_STATE_HOME", Path.home()/".local/state"))/"Codex/Logs", Path(os.environ.get("XDG_CONFIG_HOME", Path.home()/".config"))/"Codex/logs"]


def classify(source, text):
    lower = text.lower()
    if any(word in lower for word in ("error submitting", "submit failed", "submission failed", "failed to send")):
        return "conversation", "message_submit_failed"
    if "stream disconnected" in lower or "responses_retry" in source:
        return "conversation", "stream_interrupted"
    if "mcp" in source.lower():
        return "mcp", "mcp_diagnostic"
    if "tools" in source.lower() or "browser-use" in source.lower():
        return "tool", "tool_diagnostic"
    return "codex", "codex_diagnostic"


class DiagnosticCollector:
    EVENT_LIMIT = 1000
    READ_LIMIT = 1024*1024
    FILE_LIMIT = 8

    def __init__(self, home):
        self.home = home
        self.roots = diagnostic_roots(home)
        self.files = {}
        self.events = deque(maxlen=self.EVENT_LIMIT)
        self.next_scan = 0
        self.read_bytes = self.trimmed = 0
        self.sql_cursor = None
        self.sql_path = None
        self.health = {"desktop": "waiting", "core": "waiting"}

    def append(self, event):
        if len(self.events) == self.EVENT_LIMIT:
            self.trimmed += 1
        self.events.append(event)

    def desktop_line(self, line):
        match = re.match(r"^(\S{20,40}) (error|warning|warn) \[([a-zA-Z0-9_.:-]{1,160})\] (.*)", line)
        if not match:
            return
        when, level, source, body = match.groups()
        try:
            date = datetime.fromisoformat(when.replace("Z", "+00:00"))
            if not date.tzinfo:
                return
            when = date.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        except ValueError:
            return
        fields = {}
        for item in re.finditer(r'\b([a-zA-Z_][a-zA-Z0-9_]*)=("(?:\\.|[^"\\])*"|[^\s]+)', body):
            key, value = item.groups()
            if key in ("conversationId", "threadId", "thread_id", "errorCode", "errorName", "method") and value != "null" and not value.startswith('"') and identifier(value):
                fields[key] = value
        thread = next((fields[key] for key in ("conversationId", "threadId", "thread_id") if key in fields and re.fullmatch(UUID, fields[key])), None)
        heading = re.split(r"\s[a-zA-Z_][a-zA-Z0-9_]*=", body, maxsplit=1)[0]
        category, code = classify(source, heading[:512])
        self.append({"timestamp": when, "category": category, "code": fields.get("errorCode") or code, "reason": code, "error_type": fields.get("errorName"), "method": fields.get("method"), "source": "codex_desktop", "module": source, "thread_id": thread, "severity": "error" if level == "error" else "warning"})

    def refresh_desktop(self):
        if time.monotonic() >= self.next_scan:
            self.next_scan = time.monotonic()+15
            paths = []
            unavailable = False
            for root in self.roots:
                try:
                    for path in root.rglob("codex-desktop-*.log"):
                        if path.is_file():
                            paths.append((path.stat().st_mtime_ns, str(path), path))
                except OSError:
                    unavailable = True
            active = [path for _, _, path in sorted(paths, reverse=True)[:self.FILE_LIMIT]]
            self.files = {path: self.files.get(path, {"offset": None, "buffer": b"", "discard": False}) for path in active}
            self.health["desktop"] = ("partly_unavailable" if active else "unavailable") if unavailable else "ok" if active else "missing"
        budget = self.READ_LIMIT
        for path, state in self.files.items():
            if budget <= 0:
                break
            try:
                size = path.stat().st_size
                if state["offset"] is None or size < state["offset"]:
                    state.update(offset=max(0, size-256*1024), buffer=b"", discard=size > 256*1024)
                with path.open("rb") as stream:
                    stream.seek(state["offset"])
                    raw = stream.read(budget)
                state["offset"] += len(raw)
                budget -= len(raw)
                self.read_bytes += len(raw)
                lines = (state["buffer"]+raw).split(b"\n")
                state["buffer"] = lines.pop()
                if state["discard"] and lines:
                    lines.pop(0)
                    state["discard"] = False
                for line in lines:
                    if len(line) <= 65536:
                        self.desktop_line(line.decode("utf-8", errors="replace"))
                if len(state["buffer"]) > 65536:
                    state.update(buffer=b"", discard=True)
            except OSError:
                self.health["desktop"] = "partly_unavailable"

    def refresh_core(self):
        try:
            paths = list(self.home.glob("logs_*.sqlite"))
            if not paths:
                self.health["core"] = "missing"
                return
            path = max(paths, key=lambda p:p.stat().st_mtime_ns)
            if path != self.sql_path:
                self.sql_path, self.sql_cursor = path, None
            with closing(sqlite3.connect(path.as_uri()+"?mode=ro", uri=True, timeout=.08)) as db:
                db.execute("PRAGMA query_only=ON")
                columns = {row[1] for row in db.execute("PRAGMA table_info(logs)")}
                if not {"id", "ts", "level", "target", "thread_id", "feedback_log_body"} <= columns:
                    self.health["core"] = "unsupported"
                    return
                # Read a bounded ID range, not a full-body search through log history
                high = db.execute("SELECT max(id) FROM logs").fetchone()[0]
                if high is None or self.sql_cursor == high:
                    self.health["core"] = "ok"
                    return
                if self.sql_cursor is not None and high < self.sql_cursor:
                    self.sql_cursor = None
                rows = db.execute("SELECT id,ts,level,target,thread_id,CASE WHEN level IN ('ERROR','WARN') THEN substr(feedback_log_body,1,8192) END FROM logs WHERE id>? ORDER BY id DESC LIMIT 2000", (self.sql_cursor or 0,)).fetchall()
                for identity, ts, level, source, thread, body in reversed(rows):
                    if level not in ("ERROR", "WARN") or not identifier(source):
                        continue
                    try:
                        when = datetime.fromtimestamp(ts, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
                    except (ValueError, TypeError, OSError, OverflowError):
                        continue
                    category, code = classify(source, body or "")
                    self.append({"timestamp": when, "category": category, "code": code, "reason": code, "source": "codex_core", "module": source, "thread_id": thread if isinstance(thread, str) and re.fullmatch(UUID, thread) else None, "severity": "error" if level == "ERROR" else "warning"})
                self.sql_cursor = high
                self.health["core"] = "ok"
        except (OSError, sqlite3.Error):
            self.health["core"] = "unavailable"

    def refresh(self):
        self.refresh_desktop()
        self.refresh_core()

    def snapshot(self):
        return {"events": sorted(self.events, key=lambda e:e["timestamp"], reverse=True), "health": dict(self.health), "read_bytes": self.read_bytes, "trimmed": self.trimmed, "limit": self.EVENT_LIMIT}


def error_summary(events):
    events.sort(key=lambda e:e.get("timestamp") or "", reverse=True)
    return {"events": events[:1000], "total": len(events), "categories": dict(Counter(e["category"] for e in events)), "severities": dict(Counter(e["severity"] for e in events)), "limit": 1000}
