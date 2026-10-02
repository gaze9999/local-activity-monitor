"""Read selected metadata only; never return raw records to HTTP callers."""
from __future__ import annotations

from collections import Counter
from contextlib import closing
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import re
import sqlite3
import time

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\Z")
UUID_IN_NAME = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
NAME = re.compile(r"[a-zA-Z0-9_.:-]{1,160}\Z")
COUNTERS = ("input_tokens", "cached_input_tokens", "cache_write_input_tokens", "output_tokens", "reasoning_output_tokens", "total_tokens")
WINDOWS = {"1h": 1, "24h": 24, "7d": 168, "all": None}


def now():
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def integer(value):
    return value if type(value) is int and 0 <= value <= 2**63 - 1 else None


def name(value):
    return value if isinstance(value, str) and NAME.fullmatch(value) else None


def timestamp(value):
    if not isinstance(value, str) or len(value) > 40:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        return parsed.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z") if parsed.tzinfo else None
    except ValueError:
        return None


def monitor_config(home):
    path = home / "monitoring/jev-monitor.json"
    try:
        if path.stat().st_size > 8192:
            raise ValueError()
        data = json.loads(path.read_text(encoding="utf-8"))
        if data.get("version") != 1 or type(data.get("enabled")) is not bool:
            raise ValueError()
        database = Path(data["database"]).expanduser()
        if not database.is_absolute():
            raise ValueError()
        return data["enabled"], database, timestamp(data.get("enabled_at"))
    except FileNotFoundError:
        return False, None, None
    except (OSError, ValueError, TypeError, KeyError):
        return False, None, None


class JevCollector:
    def __init__(self, home):
        self.home = home

    def snapshot(self, window="24h"):
        enabled, database, since = monitor_config(self.home)
        result = {"source": "jev", "enabled": enabled, "enabled_at": since, "health": "waiting" if enabled else "disabled", "scope": "completed_operations_in_local_database", "window": window, "summary": {}, "recent": [], "series": []}
        if database is None or not database.is_file():
            return result
        hours = WINDOWS[window]
        cutoff = (datetime.now(timezone.utc) - timedelta(hours=hours)).isoformat(timespec="milliseconds").replace("+00:00", "Z") if hours else ""
        try:
            with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=.08)) as db:
                db.execute("PRAGMA query_only=ON")
                row = db.execute("SELECT count(*), min(timestamp), sum(http_attempts), sum(max(http_attempts-1,0)), sum(input_tokens), sum(output_tokens), count(input_tokens), count(output_tokens), sum(request_bytes), sum(response_bytes), sum(response_unknown_attempts), avg(latency_ms) FROM jev_events WHERE timestamp>=?", (cutoff,)).fetchone()
                count = row[0]
                result["summary"] = dict(calls=count, since=timestamp(row[1]), http_attempts=row[2] or 0, retries=row[3] or 0, input_tokens=row[4], output_tokens=row[5], input_known_calls=row[6], output_known_calls=row[7], input_unknown_calls=count-row[6], output_unknown_calls=count-row[7], request_body_bytes=row[8] or 0, known_response_bytes=row[9] or 0, response_unknown_attempts=row[10] or 0, average_latency_ms=round(row[11] or 0))
                result["summary"]["statuses"] = {status: n for status, n in db.execute("SELECT status,count(*) FROM jev_events WHERE timestamp>=? GROUP BY status", (cutoff,)) if status in ("ok", "fallback", "skipped", "dry_run")}
                for when, n in db.execute("SELECT substr(timestamp,1,13),count(*) FROM jev_events WHERE timestamp>=? GROUP BY 1 ORDER BY 1 DESC LIMIT 168", (cutoff,)):
                    if isinstance(when, str) and re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d", when):
                        result["series"].append({"time": when+":00:00Z", "calls": n})
                result["series"].reverse()
                for raw, in db.execute("SELECT metadata FROM jev_events WHERE timestamp>=? ORDER BY timestamp DESC LIMIT 80", (cutoff,)):
                    if len(raw) > 65536:
                        continue
                    try:
                        event = json.loads(raw)
                        status, operation = event.get("status"), event.get("operation")
                        if status not in ("ok", "fallback", "skipped", "dry_run") or operation not in ("rank", "evaluate", "doctor_local", "doctor_online"):
                            continue
                        attempts = event.get("attempts", [])
                        attempts = attempts if isinstance(attempts, list) else []
                        result["recent"].append({"timestamp": timestamp(event.get("timestamp")), "operation": operation, "status": status, "source": event.get("source") if event.get("source") in ("mcp", "cli", "python") else "python", "model": name(event.get("resolved_model")) or name(event.get("requested_model")), "input_tokens": integer(event.get("input_tokens")), "output_tokens": integer(event.get("output_tokens")), "latency_ms": integer(event.get("latency_ms")), "http_attempts": len(attempts), "attempts": [{"status": a.get("status") if a.get("status") in ("ok", "http_error", "network_unavailable", "invalid_response", "response_too_large") else "other", "http_status": integer(a.get("http_status")), "latency_ms": integer(a.get("latency_ms"))} for a in attempts[:2] if isinstance(a, dict)]})
                    except (ValueError, TypeError, AttributeError):
                        continue
            result["health"] = "ok"
        except (OSError, sqlite3.Error):
            result["health"] = "unavailable"
        return result


class CodexCollector:
    """Bounded initial tail, incremental append reads, latest cumulative snapshots."""
    def __init__(self, root, max_files=20, tail_bytes=1024*1024):
        self.root, self.max_files, self.tail_bytes = root, max_files, tail_bytes
        self.files = {}
        self.started_at = now()
        self.next_scan = 0
        self.read_bytes = 0
        self.malformed = 0
        self.health = "waiting"

    def scan(self):
        if time.monotonic() < self.next_scan:
            return
        self.next_scan = time.monotonic() + 15
        if not self.root.is_dir():
            self.health = "missing"
            return
        paths = []
        try:
            for path in self.root.rglob("rollout-*.jsonl"):
                try:
                    paths.append((path.stat().st_mtime_ns, path))
                except OSError:
                    continue
            active = [path for _, path in sorted(paths, reverse=True)[:self.max_files]]
            self.files = {path: self.files.get(path, self.new_file(path)) for path in active}
            self.health = "ok"
        except OSError:
            self.health = "unavailable"

    def new_file(self, path):
        ids = UUID_IN_NAME.findall(path.name)
        return {"offset": None, "buffer": b"", "discard": False, "thread_id": ids[-1] if ids else None, "model": None, "tokens": {}, "token_time": "", "token_priority": 0, "updated_at": None, "status": "observed", "calls": {}, "partial_history": True, "bytes": 0}

    def consume(self, state, record):
        payload = record.get("payload")
        if not isinstance(payload, dict):
            return
        when = timestamp(record.get("timestamp"))
        kind = record.get("type")
        if when and (not state["updated_at"] or when > state["updated_at"]):
            state["updated_at"] = when
        if kind == "session_meta":
            identity = payload.get("id")
            if isinstance(identity, str) and UUID.fullmatch(identity):
                state["thread_id"] = identity
        elif kind == "turn_context":
            value = name(payload.get("model"))
            if value:
                state["model"] = value
        elif kind == "response_item" and payload.get("type") in ("function_call", "custom_tool_call"):
            tool, identity = name(payload.get("name")), payload.get("call_id") or payload.get("id")
            if tool and isinstance(identity, str) and len(identity) <= 160:
                state["calls"][identity] = {"tool": tool, "timestamp": when}
                if len(state["calls"]) > 8192:
                    del state["calls"][next(iter(state["calls"]))]
        elif kind == "event_msg" and payload.get("type") in ("task_started", "task_complete"):
            state["status"] = "running" if payload["type"] == "task_started" else "completed"
        usage, priority = None, 0
        if kind == "token_usage_record":
            usage, priority = payload.get("thread_token_usage"), 2
        elif kind == "event_msg" and payload.get("type") == "token_count":
            info = payload.get("info")
            usage, priority = info.get("total_token_usage") if isinstance(info, dict) else None, 1
        if isinstance(usage, dict) and when and (when > state["token_time"] or when == state["token_time"] and priority >= state["token_priority"]):
            clean = {key: integer(usage.get(key)) for key in COUNTERS}
            if any(value is not None for value in clean.values()):
                state.update(tokens=clean, token_time=when, token_priority=priority)

    def refresh(self):
        self.scan()
        budget = 8*1024*1024
        for path, state in self.files.items():
            if budget <= 0:
                break
            try:
                size = path.stat().st_size
                if state["offset"] is not None and size < state["offset"]:
                    self.files[path] = state = self.new_file(path)
                with path.open("rb") as stream:
                    if state["offset"] is None:
                        head = stream.readline(65536)
                        if head.endswith(b"\n"):
                            self.parse(state, head)
                        state["offset"] = max(len(head), size-self.tail_bytes)
                        state["partial_history"] = state["offset"] > len(head)
                        state["discard"] = state["partial_history"]
                    stream.seek(state["offset"])
                    raw = stream.read(min(budget, self.tail_bytes))
                    state["offset"] += len(raw)
                    state["bytes"] += len(raw)
                    budget -= len(raw)
                    self.read_bytes += len(raw)
                data = state["buffer"] + raw
                lines = data.split(b"\n")
                state["buffer"] = lines.pop()
                if state["discard"] and lines:
                    lines.pop(0)
                    state["discard"] = False
                for line in lines:
                    self.parse(state, line)
                if len(state["buffer"]) > 1024*1024:
                    state["buffer"] = b""
                    state["discard"] = True
            except OSError:
                self.health = "partly_unavailable"

    def parse(self, state, raw):
        if not raw or len(raw) > 1024*1024:
            return
        try:
            record = json.loads(raw)
            if isinstance(record, dict):
                self.consume(state, record)
        except (ValueError, RecursionError):
            self.malformed += 1

    def snapshot(self):
        threads = {}
        for state in self.files.values():
            identity = state["thread_id"]
            if not identity:
                continue
            target = threads.setdefault(identity, {"thread_id": identity, "model": None, "tokens": {}, "token_time": "", "updated_at": None, "status": "observed", "calls": {}, "partial_history": False})
            if state["token_time"] >= target["token_time"]:
                target.update(tokens=state["tokens"], token_time=state["token_time"])
            if (state["updated_at"] or "") >= (target["updated_at"] or ""):
                target.update(model=state["model"], updated_at=state["updated_at"], status=state["status"])
            target["calls"].update(state["calls"])
            target["partial_history"] |= state["partial_history"]
        rows, tools = [], Counter()
        for thread in threads.values():
            counts = Counter(call["tool"] for call in thread["calls"].values())
            tools.update(counts)
            rows.append({key: thread[key] for key in ("thread_id", "model", "tokens", "updated_at", "status", "partial_history")} | {"tool_calls": sum(counts.values()), "tools": dict(counts)})
        rows.sort(key=lambda item: item["updated_at"] or "", reverse=True)
        return {"source": "codex", "health": self.health, "started_at": self.started_at, "scope": "recent_file_tail_and_new_records", "max_files": self.max_files, "files": len(self.files), "bytes_read": self.read_bytes, "malformed_lines": self.malformed, "threads": rows, "tools": dict(tools), "observed_tool_calls": sum(tools.values())}
