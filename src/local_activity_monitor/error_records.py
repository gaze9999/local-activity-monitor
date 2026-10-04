"""Bounded error metadata from local diagnostics; never retain log bodies."""
from collections import Counter, deque
from contextlib import closing
from datetime import datetime, timedelta, timezone
import os
from pathlib import Path
import re
import sqlite3
import sys
import time

from .mcp_records import decoded_output
from .sqlite_records import sql_diagnostic

IDENTIFIER = re.compile(r"[a-zA-Z0-9_./:-]{1,160}\Z")
UUID = r"[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}"
FAILURES = {"error", "failed", "timeout", "missing_dependencies", "network_unavailable", "http_error", "invalid_response", "response_too_large"}


def identifier(value):
    return value if isinstance(value, str) and IDENTIFIER.fullmatch(value) else None


def diagnostic_details(text):
    """Extract trace identifiers and recognized failure causes, never free-form text."""
    if not isinstance(text, str):
        return {}
    text = text[:8192]
    result = {}
    pairs = {}
    for match in re.finditer(r'\b([a-zA-Z_][a-zA-Z0-9_]*)[=:]\s*("(?:\\.|[^"\\])*"|\S+)', text):
        key, item = match.groups()
        pairs[key] = item[1:-1] if item.startswith('"') else item.rstrip(',;}')
    for key, aliases in (("request_id", "request_id|requestId"), ("trace_id", "trace_id|traceId|correlationId"), ("error_type", "errorName|error_type"), ("method", "method")):
        for alias in aliases.split('|'):
            item = pairs.get(alias)
            if isinstance(item, str) and re.fullmatch(r'[a-zA-Z0-9_.:-]{1,160}', item):
                result[key] = item
                break
    heading = re.split(r'\s[a-zA-Z_][a-zA-Z0-9_]*[=:]', text, maxsplit=1)[0]
    heading = re.sub(r'"(?:\\.|[^"\\])*"', '', heading)
    match = re.search(r'\bHTTP(?:[/ ]\d\.\d)?[ :]+(\d{3})\b', heading, re.I)
    if match and 100 <= int(match[1]) <= 599:
        result["http_status"] = int(match[1])
    for key in ('http_status', 'status_code', 'status'):
        item = pairs.get(key)
        if isinstance(item, str) and re.fullmatch(r'[1-5]\d{2}', item):
            result['http_status'] = int(item)
    causes = (("rate_limit", r"rate.?limit|too many requests"), ("quota_exceeded", r"quota.{0,12}exceeded|insufficient_quota"), ("authentication_failed", r"unauthorized|authentication.{0,12}fail|invalid.{0,8}api.?key"), ("permission_denied", r"permission denied|forbidden"), ("connection_refused", r"connection refused|ECONNREFUSED"), ("timeout", r"timed? out|timeout|ETIMEDOUT"), ("stream_interrupted", r"stream disconnected|stream interrupted"), ("invalid_response", r"invalid response|invalid JSON|parse error"))
    for cause, pattern in causes:
        if re.search(pattern, heading, re.I):
            result["cause"] = cause
            break
    return result


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
    result = {"code": code or identifier(value.get("error_code")) or ("process_exit" if exit_code not in (None, 0) else identifier(status) or "tool_error"), "exit_code": exit_code}
    if isinstance(error, dict):
        result.update(diagnostic_details(error.get("message")))
        for key in ("request_id", "trace_id", "error_type"):
            item = identifier(error.get(key))
            if item:
                result[key] = item
        status = error.get("http_status", error.get("status_code"))
        if type(status) is int and 100 <= status <= 599:
            result["http_status"] = status
    return result


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
        self.logs = deque(maxlen=self.EVENT_LIMIT)
        self.sql_events = deque(maxlen=500)
        self.capture_sql = True
        self.log_trimmed = 0
        self.capture_logs = True
        self.checked_at = None
        self.stats = {key: {"read_bytes": 0, "read_lines": 0, "parsed_lines": 0, "unsupported_lines": 0, "oversized_lines": 0, "error_type": None} for key in ("desktop", "core")}
        self.stats["core"]["read_bytes"] = None
        self.file_cursor = 0
        self.next_scan = 0
        self.read_bytes = self.trimmed = 0
        self.sql_cursor = None
        self.sql_path = None
        self.sql_file_id = None
        self.sql_history = None
        self.backfill_pending = {"desktop": 0, "core": False}
        self.health = {"desktop": "waiting", "core": "waiting"}

    def append(self, event, historical=False):
        if historical and len(self.events) == self.EVENT_LIMIT and event["timestamp"] < min(item["timestamp"] for item in self.events):
            self.trimmed += 1
            return
        if len(self.events) == self.EVENT_LIMIT:
            self.trimmed += 1
            if historical:
                self.events.remove(min(self.events, key=lambda item:item["timestamp"]))
        self.events.append(event)

    def desktop_line(self, line, file=None, historical=False):
        stats = self.stats["desktop"]
        stats["read_lines"] += 1
        match = re.match(r"^(\S{20,40}) ([a-zA-Z]{1,16}) \[([a-zA-Z0-9_.:-]{1,160})\] (.*)", line)
        if not match:
            stats["unsupported_lines"] += 1
            return
        when, level, source, body = match.groups()
        level = level.lower()
        try:
            date = datetime.fromisoformat(when.replace("Z", "+00:00"))
            if not date.tzinfo:
                stats["unsupported_lines"] += 1
                return
            when = date.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        except ValueError:
            stats["unsupported_lines"] += 1
            return
        if historical and date < datetime.now(timezone.utc)-timedelta(hours=24):
            return "expired"
        fields = {}
        for item in re.finditer(r'\b([a-zA-Z_][a-zA-Z0-9_]*)=("(?:\\.|[^"\\])*"|[^\s]+)', body):
            key, value = item.groups()
            if key in ("conversationId", "threadId", "thread_id", "errorCode", "errorName", "method") and value != "null" and not value.startswith('"') and identifier(value):
                fields[key] = value
        thread = next((fields[key] for key in ("conversationId", "threadId", "thread_id") if key in fields and re.fullmatch(UUID, fields[key])), None)
        heading = re.split(r"\s[a-zA-Z_][a-zA-Z0-9_]*=", body, maxsplit=1)[0]
        category, code = classify(source, heading[:512])
        event = {"timestamp": when, "category": category, "code": fields.get("errorCode") or code, "reason": code, "error_type": fields.get("errorName"), "method": fields.get("method"), "source": "codex_desktop", "module": source, "thread_id": thread, "severity": "warning" if level in ("warn", "warning") else level}
        event.update(diagnostic_details(body), file=file)
        stats["parsed_lines"] += 1
        sql = sql_diagnostic(source, body) if self.capture_sql and not historical else None
        if sql:
            self.sql_events.append(sql | {"timestamp": when, "source": "codex_desktop", "module": source, "thread_id": thread, "file": file, "recognition": "diagnostic_log", "result": "log_error" if level in ("error", "fatal", "critical") else "log_recorded"})
        if level in ("error", "fatal", "critical", "warn", "warning"):
            self.append(event | {"severity": "warning" if level in ("warn", "warning") else "error"}, historical)
        if self.capture_logs and not historical:
            if len(self.logs) == self.EVENT_LIMIT:
                self.log_trimmed += 1
            self.logs.append(event | {"file": file})

    def refresh_desktop(self):
        stats = self.stats["desktop"]
        stats["error_type"] = None
        if time.monotonic() >= self.next_scan:
            self.next_scan = time.monotonic()+15
            paths = []
            unavailable = False
            for root in self.roots:
                try:
                    for path in root.rglob("codex-desktop-*.log"):
                        if path.is_file():
                            paths.append((path.stat().st_mtime_ns, str(path), path))
                except OSError as error:
                    unavailable = True
                    stats["error_type"] = type(error).__name__
            active = [path for _, _, path in sorted(paths, reverse=True)[:self.FILE_LIMIT]]
            self.files = {path: self.files.get(path, {"offset": None, "buffer": b"", "discard": False}) for path in active}
            self.health["desktop"] = ("partly_unavailable" if active else "unavailable") if unavailable else "ok" if active else "missing"
            self.desktop_scan_health = self.health["desktop"]
        self.health["desktop"] = getattr(self, "desktop_scan_health", self.health["desktop"])
        budget = self.READ_LIMIT
        files = list(self.files.items())
        start = self.file_cursor % max(1, len(files))
        self.file_cursor += 1
        for path, state in files[start:]+files[:start]:
            if budget <= 0:
                break
            try:
                stat = path.stat()
                size, file_id = stat.st_size, (stat.st_dev, stat.st_ino)
                if state["offset"] is None or size < state["offset"] or state.get("file_id") != file_id:
                    state.update(offset=max(0, size-256*1024), buffer=b"", discard=size > 256*1024)
                    state["history_cursor"] = state["offset"]
                state["file_id"] = file_id
                with path.open("rb") as stream:
                    stream.seek(state["offset"])
                    raw = stream.read(budget)
                state["offset"] += len(raw)
                state["unread_bytes"] = max(0, size-state["offset"])
                budget -= len(raw)
                self.read_bytes += len(raw)
                stats["read_bytes"] += len(raw)
                lines = (state["buffer"]+raw).split(b"\n")
                state["buffer"] = lines.pop()
                if state["discard"] and lines:
                    skipped = lines.pop(0)
                    if state.get("history_cursor"):
                        state["history_cursor"] += len(skipped)+1
                    state["discard"] = False
                for line in lines:
                    if len(line) <= 65536:
                        self.desktop_line(line.decode("utf-8", errors="replace"), path.name)
                    else:
                        stats["oversized_lines"] += 1
                if len(state["buffer"]) > 65536:
                    state.update(buffer=b"", discard=True)
                    stats["oversized_lines"] += 1
            except OSError as error:
                self.health["desktop"] = "partly_unavailable"
                stats["error_type"] = type(error).__name__
        # Backfill recent error metadata in bounded chunks, independently of live append reads
        budget = self.READ_LIMIT
        for path, state in files[start:]+files[:start]:
            end = state.get("history_cursor")
            if not end or budget <= 0:
                continue
            begin = max(0, end-min(budget, 256*1024))
            try:
                with path.open("rb") as stream:
                    stream.seek(begin)
                    raw = stream.read(end-begin)
                budget -= len(raw)
                self.read_bytes += len(raw)
                stats["read_bytes"] += len(raw)
                lines = raw.split(b"\n")
                first = lines.pop(0) if begin else b""
                state["history_cursor"] = min(end-1, begin+len(first)+1) if begin and lines else begin
                for line in reversed(lines):
                    if len(line) > 65536:
                        stats["oversized_lines"] += 1
                        continue
                    if self.desktop_line(line.decode("utf-8", errors="replace"), path.name, True) == "expired":
                        state["history_cursor"] = 0
                        break
            except OSError as error:
                self.health["desktop"] = "partly_unavailable"
                stats["error_type"] = type(error).__name__
        self.backfill_pending["desktop"] = sum(state.get("history_cursor", 0) for state in self.files.values())

    def refresh_core(self):
        stats = self.stats["core"]
        stats["error_type"] = None
        try:
            paths = list(self.home.glob("logs_*.sqlite"))
            if not paths:
                self.health["core"] = "missing"
                return
            path = max(paths, key=lambda p:p.stat().st_mtime_ns)
            stat = path.stat()
            file_id = (stat.st_dev, stat.st_ino)
            if path != self.sql_path or file_id != self.sql_file_id:
                self.sql_path, self.sql_cursor, self.sql_file_id, self.sql_history = path, None, file_id, None
            with closing(sqlite3.connect(path.as_uri()+"?mode=ro", uri=True, timeout=.08)) as db:
                db.execute("PRAGMA query_only=ON")
                columns = {row[1] for row in db.execute("PRAGMA table_info(logs)")}
                if not {"id", "ts", "level", "target"} <= columns:
                    self.health["core"] = "unsupported"
                    return
                # Read a bounded ID range, not a full-body search through log history
                high = db.execute("SELECT max(id) FROM logs").fetchone()[0]
                if high is not None and type(high) is not int:
                    self.health["core"] = "unsupported"
                    return
                if high is None:
                    self.health["core"] = "ok"
                    return
                if self.sql_cursor is not None and high < self.sql_cursor:
                    self.sql_cursor, self.sql_history = None, None
                thread_column = "thread_id" if "thread_id" in columns else "NULL"
                sql_targets = " OR lower(target) LIKE '%sql%' OR lower(target) LIKE '%database%' OR lower(target) LIKE '%state_db%'" if self.capture_sql else ""
                body_column = "CASE WHEN upper(level) IN ('ERROR','FATAL','CRITICAL','WARN','WARNING')"+sql_targets+" THEN substr(feedback_log_body,1,8192) END" if "feedback_log_body" in columns else "NULL"
                fields = f"id,ts,level,target,{thread_column},{body_column}"
                initial = self.sql_cursor is None
                rows = db.execute(f"SELECT {fields} FROM logs WHERE id>? ORDER BY id {'DESC' if initial else 'ASC'} LIMIT 2000", (self.sql_cursor or 0,)).fetchall()
                if rows:
                    self.sql_cursor = max(row[0] for row in rows)
                    if initial:
                        self.sql_history = min(row[0] for row in rows) if len(rows) == 2000 else None
                historical = db.execute(f"SELECT {fields} FROM logs WHERE id<? ORDER BY id DESC LIMIT 2000", (self.sql_history,)).fetchall() if self.sql_history else []
                if self.sql_history:
                    self.sql_history = min(row[0] for row in historical) if len(historical) == 2000 else None
                cutoff = datetime.now(timezone.utc)-timedelta(hours=24)
                for row, past in [(row, False) for row in (reversed(rows) if initial else rows)]+[(row, True) for row in historical]:
                    identity, ts, level, source, thread, body = row
                    stats["read_lines"] += 1
                    if not isinstance(level, str) or not re.fullmatch(r"[a-zA-Z]{1,16}", level) or not identifier(source):
                        stats["unsupported_lines"] += 1
                        continue
                    level = level.lower()
                    try:
                        date = datetime.fromtimestamp(ts, timezone.utc)
                        when = date.isoformat(timespec="milliseconds").replace("+00:00", "Z")
                    except (ValueError, TypeError, OSError, OverflowError):
                        stats["unsupported_lines"] += 1
                        continue
                    if past and date < cutoff:
                        self.sql_history = None
                        break
                    category, code = classify(source, body or "")
                    event = {"timestamp": when, "category": category, "code": code, "reason": code, "source": "codex_core", "module": source, "thread_id": thread if isinstance(thread, str) and re.fullmatch(UUID, thread) else None, "severity": "warning" if level in ("warn", "warning") else level, "file": path.name, "record_id": identity}
                    event.update(diagnostic_details(body))
                    stats["parsed_lines"] += 1
                    sql = sql_diagnostic(source, body) if self.capture_sql and not past else None
                    if sql:
                        self.sql_events.append(sql | {"timestamp": when, "source": "codex_core", "module": source, "thread_id": event["thread_id"], "file": path.name, "record_id": identity, "recognition": "diagnostic_log", "result": "log_error" if level in ("error", "fatal", "critical") else "log_recorded"})
                    if level in ("error", "fatal", "critical", "warn", "warning"):
                        self.append(event | {"severity": "warning" if level in ("warn", "warning") else "error"}, past)
                    if self.capture_logs and not past:
                        if len(self.logs) == self.EVENT_LIMIT:
                            self.log_trimmed += 1
                        self.logs.append(event)
                self.backfill_pending["core"] = self.sql_history is not None
                self.health["core"] = "ok"
        except (OSError, sqlite3.Error) as error:
            self.health["core"] = "unavailable"
            stats["error_type"] = type(error).__name__

    def refresh(self):
        self.refresh_desktop()
        self.refresh_core()
        self.checked_at = datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")

    def log_sources(self, enabled):
        files = [{"name": path.name, "offset": state["offset"], "pending_bytes": len(state["buffer"]), "unread_bytes": state.get("unread_bytes")} for path, state in self.files.items()]
        return [{"source": "codex_"+key if key == "core" else "codex_desktop", "health": self.health[key] if enabled else "disabled", "checked_at": self.checked_at, **self.stats[key], "backfill_pending": self.backfill_pending[key], "files": files if key == "desktop" else ([{"name": self.sql_path.name, "record_id": self.sql_cursor}] if self.sql_path else []), "read_limit": self.READ_LIMIT*2 if key == "desktop" else 4000, "read_unit": "bytes" if key == "desktop" else "rows", "history_hours": 24} for key in ("desktop", "core")]

    def snapshot(self):
        return {"events": sorted(self.events, key=lambda e:e["timestamp"], reverse=True), "health": dict(self.health), "read_bytes": self.read_bytes, "trimmed": self.trimmed, "limit": self.EVENT_LIMIT}


def error_summary(events):
    events.sort(key=lambda e:e.get("timestamp") or "", reverse=True)
    return {"events": events[:1000], "total": len(events), "categories": dict(Counter(e["category"] for e in events)), "severities": dict(Counter(e["severity"] for e in events)), "limit": 1000}
