"""Read selected metadata only; never return raw records to HTTP callers."""
from __future__ import annotations
from .thread_state import ThreadState
from .activity_windows import cutoff, contains

from collections import Counter
from contextlib import closing
from datetime import datetime, timedelta, timezone
import json
import hashlib
from pathlib import Path
import re
import sqlite3
import time

from .codex_metadata import execution_metadata, read_metadata
from .codex_schedules import read_schedules
from .payload_detail import bounded_payload, complete_payload
from .error_records import identifier, tool_error
from .mcp_records import mcp_operations, response_metadata, decoded_output
from .operation_records import file_operations, git_commands, shell_parts, invocations, operations, qualified_tool, redact, workflow_operations
from .sqlite_records import sql_content, sqlite_operations
from .usage_records import allowance

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
    except (OSError, ValueError, TypeError, KeyError, AttributeError):
        return False, None, None


class JevCollector:
    RECENT_LIMIT = 80
    SERIES_LIMIT = 168
    SCOPE = "completed_operations_in_local_database"
    def __init__(self, home):
        self.home = home

    def snapshot(self, window="24h"):
        enabled, database, since = monitor_config(self.home)
        result = {"source": "jev", "enabled": enabled, "enabled_at": since, "health": "waiting" if enabled else "disabled", "scope": self.SCOPE, "window": window, "summary": {}, "recent": [], "series": []}
        if database is None and (self.home/"monitoring/jev-monitor.json").exists():
            result["health"] = "invalid_config"
        if database is None or not database.is_file():
            return result
        hours = WINDOWS[window]
        cutoff = (datetime.now(timezone.utc) - timedelta(hours=hours)).isoformat(timespec="milliseconds").replace("+00:00", "Z") if hours else ""
        try:
            with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=.08)) as db:
                db.execute("PRAGMA query_only=ON")
                row = db.execute("SELECT count(*), min(timestamp), sum(http_attempts), sum(max(http_attempts-1,0)), sum(input_tokens), sum(output_tokens), count(input_tokens), count(output_tokens), sum(request_bytes), sum(response_bytes), sum(response_unknown_attempts), avg(latency_ms) FROM jev_events WHERE timestamp>=?", (cutoff,)).fetchone()
                count = row[0]
                result["summary"] = dict(calls=count, since=timestamp(row[1]), http_attempts=row[2] or 0, retries=row[3] or 0, input_tokens=row[4], output_tokens=row[5], input_known_calls=row[6], output_known_calls=row[7], input_unknown_calls=count-row[6], output_unknown_calls=count-row[7], request_body_bytes=row[8] or 0, known_response_bytes=row[9] or 0, response_unknown_attempts=row[10] or 0, average_latency_ms=round(row[11]) if row[11] is not None else None)
                result["summary"]["statuses"] = {status: n for status, n in db.execute("SELECT status,count(*) FROM jev_events WHERE timestamp>=? GROUP BY status LIMIT 40", (cutoff,)) if name(status)}
                for when, n in db.execute(f"SELECT substr(timestamp,1,13),count(*) FROM jev_events WHERE timestamp>=? GROUP BY 1 ORDER BY 1 DESC LIMIT {self.SERIES_LIMIT}", (cutoff,)):
                    if isinstance(when, str) and re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d", when):
                        result["series"].append({"time": when+":00:00Z", "calls": n})
                result["series"].reverse()
                for raw, in db.execute(f"SELECT metadata FROM jev_events WHERE timestamp>=? ORDER BY timestamp DESC LIMIT {self.RECENT_LIMIT}", (cutoff,)):
                    if not isinstance(raw, str) or len(raw) > 65536:
                        continue
                    try:
                        event = json.loads(raw)
                        status, operation = event.get("status"), event.get("operation")
                        if not name(status) or not name(operation):
                            continue
                        attempts = event.get("attempts", [])
                        attempts = attempts if isinstance(attempts, list) else []
                        result["recent"].append({"timestamp": timestamp(event.get("timestamp")), "operation": operation, "status": status, "source": name(event.get("source")), "model": name(event.get("resolved_model")) or name(event.get("requested_model")), "input_tokens": integer(event.get("input_tokens")), "output_tokens": integer(event.get("output_tokens")), "latency_ms": integer(event.get("latency_ms")), "request_bytes": integer(event.get("request_bytes")), "response_bytes": integer(event.get("response_bytes")), "http_attempts": len(attempts), "attempts": [{"status": name(a.get("status")) or "unknown", "http_status": integer(a.get("http_status")), "latency_ms": integer(a.get("latency_ms"))} for a in attempts[:16] if isinstance(a, dict)]})
                        result["recent"][-1].update({key: value for key, value in response_metadata("jev", event).items() if type(value) in (int, float, bool)})
                    except (ValueError, TypeError, AttributeError):
                        continue
            result["health"] = "ok"
        except (OSError, sqlite3.Error):
            result["health"] = "unavailable"
        return result


class CodexCollector:
    """Bounded initial tail, incremental append reads, latest cumulative snapshots."""
    FILE_LIMIT = 5000
    CALL_LIMIT = 50000
    BUFFER_LIMIT = 8*1024*1024
    READ_LIMIT = 8*1024*1024
    SOURCE_LOCATION_LIMIT = 20
    SCAN_INTERVAL = 15
    SQL_EVENT_LIMIT = 500
    GIT_EVENT_LIMIT = 500
    CHECK_EVENT_LIMIT = 500
    FILE_EVENT_LIMIT = 1000

    def __init__(self, root, max_files=20, tail_bytes=1024*1024):
        self.root, self.max_files, self.tail_bytes = root, max_files, tail_bytes
        self.files = {}
        self.started_at = now()
        self.next_scan = 0
        self.read_bytes = 0
        self.malformed = 0
        self.health = "waiting"
        self.track_all = False
        self.features = {"usage": True, "metadata": True, "git": True, "jev_calls": True, "skills": True, "checks": True, "tool_events": True, "mcp": True, "web": True, "files": True, "errors": True, "sqlite": True}
        self.mcp_sources = {}
        self.trimmed_calls = 0
        self.read_cursor = 0
        self.thread_state = ThreadState((root.parent if root.name == "sessions" else root)/"monitoring/thread-state.json")

    def scan(self):
        if time.monotonic() < self.next_scan:
            return
        self.next_scan = time.monotonic() + self.SCAN_INTERVAL
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
            ordered = sorted(paths, reverse=True)
            active = [path for _, path in ordered[:self.FILE_LIMIT if self.track_all else self.max_files]]
            self.files = {path: self.files.get(path, self.new_file(path)) for path in active}
            self.health = "ok"
        except OSError:
            self.health = "unavailable"

    def new_file(self, path):
        ids = UUID_IN_NAME.findall(path.name)
        return {"source_file": path.name, "offset": None, "buffer": b"", "discard": False, "thread_id": ids[-1] if ids else None, "model": None, "reasoning_effort": None, "execution": {}, "context_time": "", "tokens": {}, "token_time": "", "token_priority": 0, "created_at": None, "updated_at": None, "status": "observed", "activity_type": "unknown", "trigger": "unknown", "calls": {}, "partial_history": True, "bytes": 0, "task_start": None, "task_intervals": {}, "errors": [], "task_time": "", "history_cursor": None}

    def consume(self, state, record):
        payload = record.get("payload")
        if not isinstance(payload, dict):
            return
        when = timestamp(record.get("timestamp"))
        kind = record.get("type")
        if when and (not state["updated_at"] or when > state["updated_at"]):
            state["updated_at"] = when
        if self.features["errors"] and kind == "event_msg":
            event_type = identifier(payload.get("type"))
            if event_type and (event_type in ("error", "turn_aborted", "turn_failed") or event_type.endswith("_failed")):
                error = payload.get("error")
                code = identifier(error.get("code")) if isinstance(error, dict) else None
                state["errors"].append({"timestamp": when, "category": "conversation", "code": code or identifier(payload.get("error_code")) or event_type, "reason": event_type, "file": state.get("source_file"), "source": "session", "severity": "warning" if event_type == "turn_aborted" else "error"})
                state["errors"] = sorted(state["errors"], key=lambda event:event["timestamp"] or "", reverse=True)[:50]
        if kind == "session_meta":
            identity = payload.get("id")
            if isinstance(identity, str) and UUID.fullmatch(identity):
                state["thread_id"] = identity
                state["created_at"] = when
            state["activity_type"] = "work" if payload.get("originator") == "codex_work_desktop" else "codex" if payload.get("originator") == "Codex Desktop" else "unknown"
            if payload.get("thread_source") in ("user", "subagent", "guardian_review", "dot", "orbit", "automation", "schedule", "heartbeat"):
                state["trigger"] = payload["thread_source"]
            state["execution"].update(execution_metadata(payload))
            git = payload.get("git")
            if isinstance(git, dict):
                state["execution"].update(execution_metadata({"git_branch": git.get("branch"), "git_sha": git.get("commit_hash")}))
        elif kind == "turn_context" or kind == "event_msg" and payload.get("type") == "thread_settings_applied":
            context = payload if kind == "turn_context" else payload.get("thread_settings")
            matches = kind == "turn_context" or not payload.get("thread_id") or payload.get("thread_id") == state["thread_id"]
            if isinstance(context, dict) and matches and (not state["context_time"] or when and when >= state["context_time"]):
                state["model"] = name(context.get("model"))
                state["reasoning_effort"] = name(context.get("reasoning_effort", context.get("effort")))
                state["execution"].update(execution_metadata(context))
                state["context_time"] = when or ""
        elif kind == "response_item" and payload.get("type") in ("function_call", "custom_tool_call"):
            tool, identity = name(qualified_tool(payload)), payload.get("call_id") or payload.get("id")
            if tool and isinstance(identity, str) and len(identity) <= 160:
                calls = invocations(payload) if any(self.features[key] for key in ("git", "jev_calls", "skills", "checks", "tool_events", "mcp", "web", "files", "sqlite")) else []
                git, jev = operations(payload, self.features["git"], self.features["jev_calls"], calls)
                skills, checks = workflow_operations(payload, self.features["skills"], self.features["checks"], calls, include_paths=True)
                previous = state["calls"].get(identity, {})
                mcp = [event for event in mcp_operations(calls, self.features["web"]) if event["server"] == "web" or self.features["mcp"]] if self.features["mcp"] or self.features["web"] else []
                for event in mcp:
                    if self.mcp_sources.get(event["server"]) is False:
                        event["metadata"] = {}
                state["calls"][identity] = {"tool": tool, "timestamp": when, "completed_at": previous.get("completed_at"), "error": previous.get("error"), "nested_tools": dict(Counter(tool for tool, _, nested in calls if nested and name(tool))) if self.features["tool_events"] else {}, "git": git if self.features["git"] else [], "skills": skills if self.features["skills"] else [], "checks": checks if self.features["checks"] else [], "jev": [{"operation": item["operation"], "nested": item["nested"]} for item in jev] if self.features["jev_calls"] else [], "mcp": mcp, "isolated": len(calls) == 1}
                state["calls"][identity]["files"] = file_operations(calls) if self.features["files"] else []
                state["calls"][identity]["sqlite"] = sqlite_operations(calls) if self.features["sqlite"] else []
                if len(state["calls"]) > 8192:
                    del state["calls"][next(iter(state["calls"]))]
                    self.trimmed_calls += 1
        elif kind == "response_item" and payload.get("type") in ("function_call_output", "custom_tool_call_output"):
            identity = payload.get("call_id")
            if isinstance(identity, str) and identity in state["calls"]:
                call = state["calls"][identity]
                call["completed_at"] = when
                if self.features['files'] and call.get('isolated') and len(call.get('files', []))==1:
                    output = decoded_output(payload.get('output'))
                    for source, target in (('bytes_read', 'read_bytes'), ('bytes_written', 'write_bytes')):
                        value = output.get(source)
                        if type(value) is int and 0<=value<=2**53:
                            call['files'][0][target] = value
                if self.features["errors"] or self.features["sqlite"]:
                    call["error"] = tool_error(payload.get("output"))
                if call.get("isolated") and len(call.get("mcp", [])) == 1:
                    event = call["mcp"][0]
                    if self.features["web" if event["server"] == "web" else "mcp"] and self.mcp_sources.get(event["server"], True):
                        event["result"] = response_metadata(event["server"], payload.get("output"))
        elif kind == "event_msg" and payload.get("type") in ("task_started", "task_complete"):
            latest = when and when >= state["task_time"]
            if latest:
                state["status"] = "running" if payload["type"] == "task_started" else "completed"
                state["task_time"] = when
                state["status_cached"] = False
            if payload["type"] == "task_started" and when and latest:
                state["task_start"] = timestamp(payload.get("started_at")) or when
            elif payload["type"] == "task_complete" and when:
                start = timestamp(payload.get("started_at")) or state["task_start"]
                end = timestamp(payload.get("completed_at")) or when
                if start and end >= start:
                    state["task_intervals"][start] = end
                    if len(state["task_intervals"]) > 1024:
                        del state["task_intervals"][next(iter(state["task_intervals"]))]
                if latest:
                    state["task_start"] = None
            if latest:
                state["history_cursor"] = None
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
        if kind == "event_msg" and payload.get("type") == "token_count" and isinstance(payload.get("info"), dict):
            state["execution"].update(execution_metadata({"context_window": payload["info"].get("model_context_window")}))
        if self.features["usage"] and kind == "event_msg" and payload.get("type") == "token_count" and when and when >= (state.get("allowance") or {}).get("updated_at", ""):
            limits = allowance(payload.get("rate_limits"), when)
            if limits:
                state["allowance"] = limits

    def refresh(self):
        self.scan()
        budget = self.READ_LIMIT
        entries = list(self.files.items())
        start = self.read_cursor % max(1, len(entries))
        for index, (path, state) in enumerate(entries[start:]+entries[:start]):
            if budget < 65536:
                break
            self.read_cursor = start+index+1
            try:
                size = path.stat().st_size
                if state["offset"] is not None and size < state["offset"]:
                    self.files[path] = state = self.new_file(path)
                with path.open("rb") as stream:
                    if state["offset"] is None:
                        head = stream.readline(65536)
                        self.read_bytes += len(head)
                        state["bytes"] += len(head)
                        budget -= len(head)
                        if head.endswith(b"\n"):
                            self.parse(state, head)
                        state["offset"] = max(len(head), size-self.tail_bytes)
                        state["partial_history"] = state["offset"] > len(head)
                        cached = self.thread_state.entries.get(path.name)
                        if cached and cached['thread_id'] == state['thread_id'] and cached['offset'] <= size:
                            state['cached_status'] = cached
                            if state['offset'] <= cached['offset']:
                                for key in ('task_time', 'task_start', 'status'):
                                    state[key] = cached[key]
                                state['status_cached'] = True
                        state["error_cursor"] = state["offset"] if state["partial_history"] and self.features["errors"] else None
                        if state["partial_history"] and not state["task_time"]:
                            state["history_cursor"] = state["offset"]
                        state["discard"] = state["partial_history"]
                    stream.seek(state["offset"])
                    raw = stream.read(max(0, min(budget, self.tail_bytes)))
                    state["offset"] += len(raw)
                    state["bytes"] += len(raw)
                    budget -= len(raw)
                    self.read_bytes += len(raw)
                data = state["buffer"] + raw
                lines = data.split(b"\n")
                state["buffer"] = lines.pop()
                if state["discard"] and lines:
                    skipped = lines.pop(0)
                    if state["history_cursor"] is not None:
                        state["history_cursor"] += len(skipped)+1
                    if state.get("error_cursor") is not None:
                        state["error_cursor"] += len(skipped)+1
                    state["discard"] = False
                for line in lines:
                    self.parse(state, line)
                if len(state["buffer"]) > 1024*1024:
                    state["buffer"] = b""
                    state["discard"] = True
            except OSError:
                self.health = "partly_unavailable"
        for path, state in self.files.items():
            while budget > 0 and state["history_cursor"]:
                end = state["history_cursor"]
                start = max(0, end-min(budget, 1024*1024))
                try:
                    with path.open("rb") as stream:
                        stream.seek(start)
                        raw = stream.read(end-start)
                except OSError:
                    self.health = "partly_unavailable"
                    break
                budget -= len(raw)
                self.read_bytes += len(raw)
                state["bytes"] += len(raw)
                lines = raw.split(b"\n")
                first = lines.pop(0) if start else b""
                state["history_cursor"] = start+len(first)+1 if start and lines else start
                if state["history_cursor"] >= end:
                    state["history_cursor"] = start
                for line in reversed(lines):
                    if len(line) <= 1024*1024 and (b'"task_started"' in line or b'"task_complete"' in line):
                        try:
                            record = json.loads(line)
                            if isinstance(record, dict) and record.get("type") == "event_msg" and isinstance(record.get("payload"), dict) and record["payload"].get("type") in ("task_started", "task_complete"):
                                self.consume(state, record)
                                if state["task_time"]:
                                    break
                        except (ValueError, RecursionError):
                            continue
                if state["task_time"] or not raw:
                    state["history_cursor"] = None
        if self.features["errors"]:
            cutoff = datetime.now(timezone.utc)-timedelta(hours=24)
            for path, state in entries[start:]+entries[:start]:
                end = state.get("error_cursor")
                if not end or budget <= 0:
                    continue
                begin = max(0, end-min(budget, 256*1024))
                try:
                    with path.open("rb") as stream:
                        stream.seek(begin)
                        raw = stream.read(end-begin)
                except OSError:
                    self.health = "partly_unavailable"
                    continue
                budget -= len(raw)
                self.read_bytes += len(raw)
                state["bytes"] += len(raw)
                lines = raw.split(b"\n")
                first = lines.pop(0) if begin else b""
                state["error_cursor"] = min(end-1, begin+len(first)+1) if begin and lines else begin
                for line in reversed(lines):
                    if not line or len(line) > 1024*1024:
                        continue
                    try:
                        record = json.loads(line)
                        if not isinstance(record, dict) or not isinstance(record.get("payload"), dict):
                            continue
                        date = datetime.fromisoformat(record.get("timestamp", "").replace("Z", "+00:00"))
                        if not date.tzinfo:
                            continue
                        if date < cutoff:
                            state["error_cursor"] = None
                            break
                        payload, kind = record["payload"], record.get("type")
                        event_type = identifier(payload.get("type"))
                        if kind == "event_msg" and event_type and (event_type in ("error", "turn_aborted", "turn_failed") or event_type.endswith("_failed")):
                            self.consume(state, record)
                        elif kind == "response_item" and payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                            failure = tool_error(payload.get("output"))
                            if failure:
                                state["errors"].append({"timestamp": record["timestamp"], "source": "tool_result", "category": "tool", "severity": "error", "call_id": identifier(payload.get("call_id")), **failure})
                                state["errors"] = sorted(state["errors"], key=lambda event:event["timestamp"] or "", reverse=True)[:50]
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
        retained = sum(len(state["calls"]) for state in self.files.values())
        if retained > self.CALL_LIMIT:
            oldest = sorted((call.get("timestamp") or "", str(path), identity, state) for path, state in self.files.items() for identity, call in state["calls"].items())
            for _, _, identity, state in oldest[:retained-self.CALL_LIMIT]:
                del state["calls"][identity]
                self.trimmed_calls += 1
        retained_errors = sorted((event.get("timestamp") or "", str(path), index, state) for path, state in self.files.items() for index, event in enumerate(state["errors"]))
        drop = {}
        for _, path, index, state in retained_errors[:max(0, len(retained_errors)-1000)]:
            drop.setdefault(path, (state, []))[1].append(index)
        for state, indexes in drop.values():
            for index in sorted(indexes, reverse=True):
                del state["errors"][index]
        buffered = 0
        for state in self.files.values():
            buffered += len(state["buffer"])
            if buffered > self.BUFFER_LIMIT:
                buffered -= len(state["buffer"])
                state["buffer"], state["discard"] = b"", True

        if self.root.is_dir():
            self.thread_state.update(self.files.values())

    def parse(self, state, raw):
        if not raw or len(raw) > 1024*1024:
            return
        try:
            record = json.loads(raw)
            if isinstance(record, dict):
                self.consume(state, record)
        except (ValueError, RecursionError):
            self.malformed += 1

    def skill_detail(self, skill, document=None, thread_id=None, call_id=None):
        empty = {"skill": skill, "files": [], "files_truncated": False, "documents": []}
        paths = []
        budget = self.READ_LIMIT
        targeted = thread_id is not None or call_id is not None
        states = [(path, state) for path, state in self.files.items() if not targeted or state["thread_id"] == thread_id]
        observed = False
        for path, state in states:
            calls = state["calls"]
            for identity, call in calls.items():
                if targeted and identity != call_id:
                    continue
                for item in call.get("skills", []):
                    if item["skill"] == skill:
                        observed = True
                        if item.get("doc_path") and item not in paths:
                            paths.append(item)
            retained = any(event["thread_id"] == state["thread_id"] and event["skill"] == skill and (not targeted or event["call_id"] == call_id) for event in self.thread_state.skills)
            observed |= retained
            if retained and budget > 0 and not path.is_symlink() and (not targeted or not paths):
                try:
                    with path.open("rb") as stream:
                        stream.seek(0, 2)
                        limit = min(self.tail_bytes, budget)
                        offset = max(0, stream.tell()-limit)
                        stream.seek(offset)
                        tail = stream.read(limit)
                        budget -= len(tail)
                    if offset:
                        tail = tail.partition(b"\n")[2]
                    identities = {event["call_id"] for event in self.thread_state.skills if event["thread_id"] == state["thread_id"] and event["skill"] == skill}
                    for raw in tail.splitlines():
                        try:
                            record = json.loads(raw)
                            payload = record.get("payload") if isinstance(record, dict) else None
                            if record.get("type") != "response_item" or not isinstance(payload, dict) or payload.get("type") not in ("function_call", "custom_tool_call") or (payload.get("call_id") or payload.get("id")) not in identities or targeted and (payload.get("call_id") or payload.get("id")) != call_id:
                                continue
                            items, _ = workflow_operations(payload, include_checks=False, include_paths=True)
                            for item in items:
                                if item["skill"] == skill and item.get("doc_path") and item not in paths:
                                    paths.append(item)
                        except (ValueError, TypeError, AttributeError, RecursionError):
                            continue
                except OSError:
                    continue
        if not observed:
            return empty
        # Name-only compatibility requests must not choose between different origins.
        if not targeted and len({(item["doc_path"], item.get("workdir")) for item in paths}) > 1:
            return empty
        for item in reversed(paths[-16:]):
            try:
                raw = item["doc_path"]
                if raw.startswith(("//", "\\\\")):
                    continue
                path = Path(raw).expanduser()
                if not path.is_absolute():
                    if not item.get("workdir") or not Path(item["workdir"]).is_absolute():
                        continue
                    path = Path(item["workdir"])/path
                path = path.resolve()
                if path.name.lower() != "skill.md" or not path.is_file():
                    continue
                root = path.parent
                files, pending, visited, truncated = {}, [(root, 0)], 0, False

                def describe(candidate):
                    try:
                        target = candidate.resolve()
                        if candidate.is_symlink() or not target.is_relative_to(root) or not target.is_file():
                            return
                        relative = candidate.relative_to(root).as_posix()
                        if any(part.startswith(".") for part in candidate.relative_to(root).parts) or candidate.name.lower() in ("auth.json", "credentials.json", "secrets.json") or candidate.suffix.lower() in (".pem", ".key", ".pfx", ".p12"):
                            return
                        stat = target.stat()
                        suffix = target.suffix.lower()
                        readable = suffix in (".md", ".markdown", ".mdown", ".txt", ".rst", ".adoc", ".asciidoc") or target.name.lower() in ("readme", "license", "changelog")
                        files[relative] = {"name": candidate.name, "relative_path": relative, "path": str(target), "kind": "document" if readable else "python" if suffix in (".py", ".pyw") else "file", "format": suffix.lstrip("."), "readable": readable, "bytes": stat.st_size, "modified_at": datetime.fromtimestamp(stat.st_mtime, timezone.utc).isoformat().replace("+00:00", "Z")}
                    except (OSError, ValueError, RuntimeError):
                        return

                # Primary documents stay available even when the inventory is capped
                describe(path)
                describe(root/"README.md")
                while pending and visited <= 1000 and len(files) < 200:
                    directory, depth = pending.pop()
                    try:
                        for candidate in directory.iterdir():
                            visited += 1
                            if visited > 1000 or len(files) >= 200:
                                truncated = True
                                break
                            target = candidate.resolve()
                            if candidate.is_symlink() or not target.is_relative_to(root) or candidate.name.startswith("."):
                                continue
                            if target.is_dir():
                                if candidate.name in ("__pycache__", "node_modules", "venv", "dist", "build"):
                                    continue
                                if depth < 4:
                                    pending.append((candidate, depth+1))
                                else:
                                    truncated = True
                            else:
                                describe(candidate)
                    except (OSError, ValueError, RuntimeError):
                        continue
                truncated |= bool(pending)
                inventory = sorted(files.values(), key=lambda entry: (entry["relative_path"].lower() not in ("skill.md", "readme.md"), entry["relative_path"].lower()))
                selected = [files[document]] if document in files and files[document]["readable"] else []
                if document is None:
                    selected = [entry for entry in inventory if entry["relative_path"].lower() in ("skill.md", "readme.md")]
                documents = []
                for entry in selected:
                    try:
                        candidate = root/entry["relative_path"]
                        target = candidate.resolve()
                        if candidate.is_symlink() or not target.is_relative_to(root) or not target.is_file():
                            continue
                        with target.open("rb") as stream:
                            content = stream.read(262145)
                    except (OSError, ValueError, RuntimeError):
                        continue
                    limited = content[:262144]
                    text = limited.decode("utf-8-sig", errors="replace")
                    documents.append({**entry, "text": redact(text, limit=None), "read_bytes": len(limited), "truncated": len(content)>262144, "sha256": hashlib.sha256(limited).hexdigest()})
                return {"skill": skill, "root": str(root), "files": inventory, "files_truncated": truncated, "documents": documents}
            except (OSError, ValueError, RuntimeError):
                continue
        return {"skill": skill, "files": [], "files_truncated": False, "documents": []}

    def git_detail(self, thread_id, call_id, operation, full=False):
        result = {"commands": [], "output": None, "output_scope": None, "truncated": False}
        budget = self.READ_LIMIT
        for path, state in self.files.items():
            call = state["calls"].get(call_id)
            if state["thread_id"] != thread_id or not call or not any(item["operation"] == operation for item in call.get("git", [])):
                continue
            if path.is_symlink() or budget <= 0:
                continue
            try:
                with path.open("rb") as stream:
                    stream.seek(0, 2)
                    limit = min(self.tail_bytes, budget)
                    offset = max(0, stream.tell()-limit)
                    stream.seek(offset)
                    tail = stream.read(limit)
                    budget -= len(tail)
                if offset:
                    tail = tail.partition(b"\n")[2]
                commands, output, outer = [], None, True
                for raw in tail.splitlines():
                    try:
                        record = json.loads(raw)
                        payload = record.get("payload") if isinstance(record, dict) else None
                        if record.get("type") != "response_item" or not isinstance(payload, dict) or (payload.get("call_id") or payload.get("id")) != call_id:
                            continue
                        if payload.get("type") in ("function_call", "custom_tool_call"):
                            calls = invocations(payload)
                            all_parts = []
                            for tool, args, nested in calls:
                                if tool not in ("exec_command", "functions.exec_command") or not isinstance(args, dict) or not isinstance(args.get("cmd"), str):
                                    continue
                                for part in shell_parts(args["cmd"]):
                                    all_parts.append(part)
                                    if operation in git_commands(part):
                                        commands.append(part.strip())
                            outer = len(calls) != 1 or any(item[2] for item in calls) or len(all_parts) != 1
                        elif payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                            output = payload.get("output")
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
                if commands:
                    command_text = "\n".join(commands)
                    presented = (complete_payload if full else bounded_payload)(output)
                    result.update(commands=[redact(command_text if full else command_text[:32768], limit=None if full else 32768)], output=presented["value"], output_scope="outer_scope" if outer else "git_command", truncated=not full and len(command_text)>32768 or presented["truncated"])
                    return result
            except (OSError, ValueError, RuntimeError):
                continue
        return result

    def sql_detail(self, event, mask=True, full=False):
        for path, state in self.files.items():
            if state["thread_id"] != event.get("thread_id") or event.get("call_id") not in state["calls"]:
                continue
            try:
                with path.open("rb") as stream:
                    stream.seek(0, 2)
                    offset = max(0, stream.tell()-self.tail_bytes)
                    stream.seek(offset)
                    tail = stream.read(self.tail_bytes)
                    if offset:
                        tail = tail.partition(b"\n")[2]
                    for raw in tail.splitlines():
                        try:
                            record = json.loads(raw)
                            payload = record.get("payload") if isinstance(record, dict) else None
                            if not isinstance(payload, dict) or record.get("type") != "response_item" or payload.get("call_id") != event.get("call_id") or payload.get("type") not in ("function_call", "custom_tool_call"):
                                continue
                            calls = sqlite_operations(invocations(payload), include_sql=True)
                            index = event.get("index", -1)
                            if 0 <= index < len(calls) and calls[index]["statement"] == event["statement"]:
                                return sql_content(calls[index].get("sql"), mask, full)
                        except (ValueError, AttributeError, RecursionError):
                            continue
            except OSError:
                continue
        return sql_content(None)

    def jev_detail(self, thread_id, call_id, index, full=False):
        request, output, nested, operation, isolated = None, None, False, None, True
        for path, state in self.files.items():
            if state["thread_id"] != thread_id or full and (path.is_symlink() or not 0 <= index < len(state["calls"].get(call_id, {}).get("jev", []))):
                continue
            try:
                with path.open("rb") as stream:
                    stream.seek(0, 2)
                    offset = max(0, stream.tell()-min(self.tail_bytes, self.READ_LIMIT))
                    stream.seek(offset)
                    if offset:
                        stream.readline()
                    for raw in stream.read(min(self.tail_bytes, self.READ_LIMIT)).splitlines():
                        try:
                            record = json.loads(raw)
                            payload = record.get("payload") if isinstance(record, dict) else None
                            if record.get("type") != "response_item" or not isinstance(payload, dict) or payload.get("call_id") != call_id:
                                continue
                            if payload.get("type") in ("function_call", "custom_tool_call"):
                                _, calls = operations(payload)
                                if index < len(calls):
                                    request, nested, operation = calls[index]["arguments"], calls[index]["nested"], calls[index]["operation"]
                                    isolated = not nested or len(invocations(payload)) == 1
                            elif payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                                output = payload.get("output")
                                if isinstance(output, str):
                                    try:
                                        output = json.loads(output)
                                    except ValueError:
                                        pass
                        except (ValueError, AttributeError, RecursionError):
                            continue
            except OSError:
                continue
        if full and not operation:
            return None
        present = complete_payload if full else bounded_payload
        sent, returned = present(request if operation else None), present(output if operation and isolated else None)
        return {"operation": operation, "request": sent["value"], "response": returned["value"], "response_scope": "containing_tool_call" if nested else "jev_tool_call", "truncated": sent["truncated"] or returned["truncated"]}

    def tool_detail(self, thread_id, call_id, full=False):
        if not self.features.get('tool_events', True):
            return None
        for path, state in self.files.items():
            call = state['calls'].get(call_id)
            if state['thread_id']!=thread_id or not call or path.is_symlink():
                continue
            request, output, found = None, None, False
            try:
                with path.open('rb') as stream:
                    stream.seek(0, 2)
                    offset = max(0, stream.tell()-min(self.tail_bytes, self.READ_LIMIT))
                    stream.seek(offset)
                    body = stream.read(min(self.tail_bytes, self.READ_LIMIT))
                if offset:
                    body = body.partition(b'\n')[2]
                for line in body.splitlines():
                    try:
                        record = json.loads(line)
                        payload = record.get('payload') if isinstance(record, dict) else None
                        if record.get('type')!='response_item' or not isinstance(payload, dict) or (payload.get('call_id') or payload.get('id'))!=call_id:
                            continue
                        if payload.get('type') in ('function_call', 'custom_tool_call') and name(qualified_tool(payload))==call.get('tool'):
                            request = payload.get('arguments', payload.get('input'))
                            found = True
                        elif payload.get('type') in ('function_call_output', 'custom_tool_call_output'):
                            output = payload.get('output')
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
                if not found:
                    return None
                present = complete_payload if full else bounded_payload
                sent, returned = present(request), present(output)
                return {'tool': call.get('tool'), 'request': sent['value'], 'response': returned['value'],
                        'truncated': sent['truncated'] or returned['truncated']}
            except OSError:
                return None
        return None

    def mcp_detail(self, thread_id, call_id, index, full=False):
        for path, state in self.files.items():
            call = state["calls"].get(call_id)
            events = call.get("mcp", []) if call else []
            if state["thread_id"] != thread_id or not 0 <= index < len(events) or path.is_symlink():
                continue
            event = events[index]
            if self.mcp_sources.get(event["server"]) is False:
                return None
            request, output, observed = None, None, False
            try:
                with path.open("rb") as stream:
                    stream.seek(0, 2)
                    offset = max(0, stream.tell()-min(self.tail_bytes, self.READ_LIMIT))
                    stream.seek(offset)
                    tail = stream.read(min(self.tail_bytes, self.READ_LIMIT))
                if offset:
                    tail = tail.partition(b"\n")[2]
                for raw in tail.splitlines():
                    try:
                        record = json.loads(raw)
                        payload = record.get("payload") if isinstance(record, dict) else None
                        if record.get("type") != "response_item" or not isinstance(payload, dict) or (payload.get("call_id") or payload.get("id")) != call_id:
                            continue
                        if payload.get("type") in ("function_call", "custom_tool_call"):
                            calls = invocations(payload)
                            found = mcp_operations(calls)
                            if index >= len(found) or (found[index]["server"], found[index]["tool"]) != (event["server"], event["tool"]):
                                continue
                            eligible = [(tool, args) for tool, args, _ in calls if mcp_operations([(tool, args, False)])]
                            request, observed = eligible[index][1], True
                        elif payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                            output = payload.get("output")
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
                present = complete_payload if full else bounded_payload
                sent, returned = present(request), present(output)
                return {"request": sent["value"] if observed else None, "response": returned["value"] if observed else None,
                        "response_scope": "containing_tool_call" if event["nested"] else "mcp_tool_call",
                        "truncated": sent["truncated"] or returned["truncated"]}
            except OSError:
                continue
        return None

    def snapshot_windows(self):
        metadata_cache = {}
        reference = datetime.now(timezone.utc)
        return {window: self.snapshot(window, reference, metadata_cache) for window in WINDOWS}

    def snapshot(self, window="all", reference=None, metadata_cache=None):
        boundary = cutoff(window, reference)
        threads = {}
        for state in self.files.values():
            identity = state["thread_id"]
            if not identity:
                continue
            target = threads.setdefault(identity, {"thread_id": identity, "model": None, "reasoning_effort": None, "execution": {}, "context_time": "", "tokens": {}, "token_time": "", "created_at": None, "updated_at": None, "status": "observed", "activity_type": "unknown", "trigger": "unknown", "calls": {}, "partial_history": False, "task_intervals": {}, "task_start": None, "errors": [], "task_time": ""})
            target["task_intervals"].update(state["task_intervals"])
            cached = state.get('cached_status')
            if cached and cached['task_time'] > target.get('cached_status', {}).get('task_time', ''):
                target['cached_status'] = cached
            target["errors"].extend(state["errors"])
            if state["task_time"] > target["task_time"]:
                target["task_time"], target["status"], target["task_start"] = state["task_time"], state["status"], state["task_start"]
                target["status_cached"] = state.get("status_cached", False)
            if state["task_start"] and (not target["task_start"] or state["task_start"] > target["task_start"]):
                target["task_start"] = state["task_start"]
            if state["created_at"] and (not target["created_at"] or state["created_at"] < target["created_at"]):
                target["created_at"] = state["created_at"]
            if state["token_time"] >= target["token_time"]:
                target.update(tokens=state["tokens"], token_time=state["token_time"])
            target["execution"].update({key: value for key, value in state["execution"].items() if key not in target["execution"]})
            if state["context_time"] and state["context_time"] >= target["context_time"]:
                target.update(model=state["model"], reasoning_effort=state["reasoning_effort"], context_time=state["context_time"])
                target["execution"].update(state["execution"])
            if (state["updated_at"] or "") >= (target["updated_at"] or ""):
                target.update(updated_at=state["updated_at"], activity_type=state["activity_type"], trigger=state["trigger"])
            target["calls"].update(state["calls"])
            target["partial_history"] |= state["partial_history"]
        dots = {}
        metadata_sources = {}
        if metadata_cache is not None and "metadata" in metadata_cache:
            metadata, dots, metadata_sources, schedules = metadata_cache["metadata"]
        else:
            self.project_details = {}
            metadata = read_metadata(self.root.parent, threads, dots, metadata_sources, self.project_details) if self.features["metadata"] else {}
            schedules = read_schedules(self.root.parent, metadata_sources) if self.features["metadata"] else {"items": [], "health": "disabled"}
            if metadata_cache is not None:
                metadata_cache["metadata"] = metadata, dots, metadata_sources, schedules
        dots = dict(dots)
        dot_events = dots.pop("_retained_events", dots.get("events"))
        if dot_events is not None:
            scoped_dots = [event for event in dot_events if contains(event, boundary)]
            dots.update(events=sorted(scoped_dots, key=lambda item:item.get("timestamp") or "", reverse=True)[:500], total=len(scoped_dots))
        dots.update(window=window, activity_items_scope="latest_loaded_snapshot")
        pending_status = {state["thread_id"] for state in self.files.values() if state.get("history_cursor")}
        rows, tools, nested_tools, git_events, skill_events, check_events, series, mcp_events, tool_series = [], Counter(), Counter(), [], [], [], Counter(), [], Counter()
        file_events, error_events, sqlite_events = [], [], []
        tool_statistics = {}

        def observe_tool(tool, nested, count, thread_id, call, duration=None):
            item = tool_statistics.setdefault((tool, nested), {"tool": tool, "nested": nested, "calls": 0, "threads": set(), "returned": None if nested else 0, "durations": [], "last_at": None, "last_response_at": None})
            item["calls"] += count
            item["threads"].add(thread_id)
            when = call.get("completed_at") or call.get("timestamp")
            if when and (item["last_at"] is None or when > item["last_at"]):
                item["last_at"] = when
            if not nested:
                if call.get("completed_at"):
                    item["returned"] += 1
                    item["last_response_at"] = max(item["last_response_at"] or "", call["completed_at"])
                if duration is not None:
                    item["durations"].append(duration)
        for identity, entry in metadata.items():
            if identity not in threads:
                threads[identity] = {"thread_id": identity, "model": None, "reasoning_effort": None, "execution": {}, "tokens": {}, "token_time": "", "created_at": None, "updated_at": None, "status": "observed", "activity_type": "unknown", "trigger": "unknown", "calls": {}, "partial_history": True, "metadata_only": True}
        for thread in threads.values():
            thread["calls"] = {key: call for key, call in thread["calls"].items() if contains(call, boundary)}
            entry = metadata.get(thread["thread_id"], {})
            row = {key: thread[key] for key in ("thread_id", "model", "reasoning_effort", "tokens", "created_at", "updated_at", "status", "partial_history", "activity_type", "trigger")}
            row["execution"] = entry.get("execution", {}) | thread["execution"]
            row["metadata_only"] = bool(thread.get("metadata_only"))
            row["archived"] = entry.get("archived")
            row["project_kind"] = getattr(self, "project_details", {}).get(entry.get("project_id"), {}).get("kind")
            cached = thread.get('cached_status', {})
            if not thread.get('task_time') and not entry.get('status') and cached:
                row['status'] = cached['status']
            row['status_updated_at'] = thread.get('task_time') or (entry.get('updated_at') if entry.get('status') else cached.get('task_time'))
            row['status_source'] = ('cached_lifecycle' if thread.get('status_cached') else 'session_lifecycle') if thread.get('task_time') else 'catalog_status' if entry.get('status') else 'cached_lifecycle' if cached else None
            row["status_backfill_pending"] = thread["thread_id"] in pending_status
            row["context_updated_at"] = thread.get("context_time") or None
            environment = entry.get("environment", "unknown")
            if environment == "unknown" and not thread.get("metadata_only"):
                environment = "local"
            row.update({"thread_name": entry.get("thread_name"), "environment": environment, "project_id": entry.get("project_id"), "project_name": entry.get("project_name"), "project_scope": entry.get("project_scope", "unknown"), "has_schedule": entry.get("has_schedule", False)})
            for key in ("activity_type", "trigger", "model", "reasoning_effort", "created_at", "updated_at", "status"):
                if key in entry and (not row.get(key) or row[key] in ("unknown", "observed") or thread.get("metadata_only")):
                    row[key] = entry[key]
            if entry.get("trigger") in ("dot", "schedule", "heartbeat"):
                row["trigger"] = entry["trigger"]
            if self.features["errors"]:
                error_events.extend(event | {"thread_id": thread["thread_id"], "thread_name": row["thread_name"]} for event in thread.get("errors", []) if contains(event, boundary))
            counts = Counter(call["tool"] for call in thread["calls"].values())
            tools.update(counts)
            nested_counts = Counter()
            events, jev_calls, file_changes, file_reads = [], [], [], []
            for identity, call in thread["calls"].items():
                duration = None
                if any(self.features[key] for key in ("tool_events", "git", "checks", "mcp", "web", "files", "sqlite")) and call["timestamp"] and call.get("completed_at"):
                    duration = max(0, round((datetime.fromisoformat(call["completed_at"].replace("Z", "+00:00"))-datetime.fromisoformat(call["timestamp"].replace("Z", "+00:00"))).total_seconds()*1000))
                if self.features["errors"] and call.get("error"):
                    mcp = call.get("mcp", [])
                    event = mcp[0] if call.get("isolated") and len(mcp) == 1 else None
                    if event is None or self.mcp_sources.get(event["server"], True):
                        error_events.append(call["error"] | {"timestamp": call.get("completed_at") or call["timestamp"], "category": "mcp" if event and event["server"] != "web" else "tool", "server": event["server"] if event else None, "tool": event["tool"] if event else call["tool"], "thread_id": thread["thread_id"], "thread_name": row["thread_name"], "call_id": identity, "source": "tool_result", "severity": "error"})
                if self.features["tool_events"]:
                    nested_counts.update(call.get("nested_tools", {}))
                    observe_tool(call["tool"], False, 1, thread["thread_id"], call, duration)
                    for tool, count in call.get("nested_tools", {}).items():
                        observe_tool(tool, True, count, thread["thread_id"], call)
                    events.append({"call_id": identity, "tool": call["tool"], "nested_tools": call.get("nested_tools", {}), "timestamp": call["timestamp"], "completed_at": call.get("completed_at"), "duration_ms": duration})
                if call["timestamp"]:
                    series[call["timestamp"][:16]+":00Z"] += 1
                    tool_series[(call["timestamp"][:16]+":00Z", call["tool"])] += 1
                for index, operation in enumerate(call.get("git", []) if self.features["git"] else []):
                    git_events.append(operation | {"thread_id": thread["thread_id"], "thread_name": row["thread_name"], "call_id": identity, "index": index, "timestamp": call["timestamp"], "completed_at": call.get("completed_at"), "duration_ms": duration})
                for index, operation in enumerate(call.get("jev", []) if self.features["jev_calls"] else []):
                    jev_calls.append(operation | {"thread_id": thread["thread_id"], "call_id": identity, "index": index, "timestamp": call["timestamp"], "completed_at": call.get("completed_at")})
                context = {"thread_id": thread["thread_id"], "thread_name": row["thread_name"], "call_id": identity, "timestamp": call["timestamp"], "completed_at": call.get("completed_at"), "duration_ms": duration}
                if self.features["sqlite"]:
                    for index, operation in enumerate(call.get("sqlite", [])):
                        sqlite_events.append(context | operation | {"index": index, "duration_ms": None, "container_duration_ms": duration, "result": "failed" if call.get("error") else "returned" if call.get("completed_at") else "unknown"})
                if self.features["files"]:
                    for change in call.get("files", []):
                        event = context | change
                        if event["nested"]:
                            event["container_duration_ms"], event["duration_ms"] = duration, None
                        file_events.append(event)
                        (file_reads if change["operation"] == "read" else file_changes).append(event)
                if self.features["mcp"] or self.features["web"]:
                    for index, event in enumerate(call.get("mcp", [])):
                        if self.features["web" if event["server"] == "web" else "mcp"]:
                            mcp_events.append(event | context | {"index": index, "result": event.get("result", {}), "duration_ms": None if event["nested"] else duration, "container_duration_ms": duration if event["nested"] else None})
                if self.features["skills"]:
                    skill_events.extend(context | {"skill": skill["skill"], "tool": call["tool"], "has_document": bool(skill.get("doc_path"))} for skill in call.get("skills", []))
                if self.features["checks"]:
                    check_events.extend(context | operation for operation in call.get("checks", []))
            events.sort(key=lambda event: event["timestamp"] or "", reverse=True)
            jev_calls.sort(key=lambda event: event["timestamp"] or "", reverse=True)
            nested_tools.update(nested_counts)
            row.update(token_updated_at=thread["token_time"] or None, tool_calls=None if thread.get("metadata_only") else sum(counts.values()), tools=dict(counts), nested_tools=dict(nested_counts), tool_events=events[:100] if self.features["tool_events"] else [], jev_calls=jev_calls[:100] if self.features["jev_calls"] else [])
            row["file_changes"] = sorted(file_changes, key=lambda event: event["timestamp"] or "", reverse=True)[:200]
            row["file_reads"] = sorted(file_reads, key=lambda event: event["timestamp"] or "", reverse=True)[:200]
            intervals = dict(thread.get("task_intervals", {}))
            if thread.get("task_start") and thread["task_start"] not in intervals and row["status"] == "running":
                intervals[thread["task_start"]] = now()
            row["task_duration_ms"] = sum(max(0, round((datetime.fromisoformat(end.replace("Z", "+00:00"))-datetime.fromisoformat(start.replace("Z", "+00:00"))).total_seconds()*1000)) for start, end in intervals.items()) if intervals else None
            row["task_runs"] = len(intervals) if intervals else None
            rows.append(row)
        rows.sort(key=lambda item: item["updated_at"] or "", reverse=True)
        git_events.sort(key=lambda event: event["timestamp"] or "", reverse=True)
        if self.features['skills']:
            selected = {event['thread_id'] for event in rows}
            retained = {(event['thread_id'], event['call_id'], event['skill']):event | {'thread_name':metadata.get(event['thread_id'], {}).get('thread_name')} for event in self.thread_state.skills if event['thread_id'] in selected and contains(event, boundary)}
            for event in retained.values():
                call = threads.get(event['thread_id'], {}).get('calls', {}).get(event['call_id'])
                if call is not None:
                    event['tool'] = call['tool']
            retained.update({(event['thread_id'], event['call_id'], event['skill']):event for event in skill_events})
            skill_events = sorted(retained.values(), key=lambda event:event['timestamp'] or '', reverse=True)
        skill_summary = {"events": skill_events[:self.thread_state.SKILL_LIMIT] if self.features["skills"] else [], "counts": dict(Counter(event["skill"] for event in skill_events)) if self.features["skills"] else {}, "total": len(skill_events) if self.features["skills"] else 0}
        check_events.sort(key=lambda event: event["timestamp"] or "", reverse=True)
        file_events.sort(key=lambda event: event["timestamp"] or "", reverse=True)
        sqlite_events.sort(key=lambda event: event["timestamp"] or "", reverse=True)
        sqlite_summary = {"events": sqlite_events[:self.SQL_EVENT_LIMIT], "total": len(sqlite_events), "operations": dict(Counter(event["operation"] for event in sqlite_events))}
        if metadata_cache is not None:
            sqlite_summary["_retained_events"] = sqlite_events
        statistics = []
        for _, item in sorted(tool_statistics.items()):
            durations = sorted(item.pop("durations"))
            item["thread_count"] = len(item.pop("threads"))
            item["known_duration_count"] = None if item["nested"] else len(durations)
            item["average_ms"] = round(sum(durations)/len(durations), 2) if durations else None
            item["p99_ms"] = None
            if durations:
                position = (len(durations)-1)*.99
                lower = int(position)
                item["p99_ms"] = round(durations[lower]+(durations[min(len(durations)-1, lower+1)]-durations[lower])*(position-lower), 2)
            statistics.append(item)
        projects = {project_id: {"id": project_id, "name": detail.get("name"), "icon": detail.get("icon"), "kind": detail.get("kind"), "thread_count": 0} for project_id, detail in getattr(self, "project_details", {}).items()}
        for project_id, detail in getattr(self, "project_details", {}).items():
            projects[project_id].update({key: detail[key] for key in ("origin", "created_at", "updated_at") if key in detail})
        for row in rows:
            project_id = row.get("project_id")
            if project_id:
                detail = getattr(self, "project_details", {}).get(project_id, {})
                project = projects.setdefault(project_id, {"id": project_id, "name": row.get("project_name"), "icon": detail.get("icon"), "kind": detail.get("kind"), "thread_count": 0})
                project["thread_count"] += 1
        return {"projects": list(projects.values()), "schedules": schedules, "activity_scope": {"window": window, "timestamp": "call_started_at", "unknown_timestamp": "all_only", "latest_state": ["tokens", "model", "status", "usage", "schedules"]}, "tool_statistics": statistics, "metadata_sources": list(metadata_sources.values()), "read_state": {
            "file_limit": self.FILE_LIMIT if self.track_all else self.max_files, "file_count": len(self.files),
            "read_limit": self.READ_LIMIT, "tail_bytes": self.tail_bytes, "scan_seconds": self.SCAN_INTERVAL,
            "call_limit": self.CALL_LIMIT, "buffer_limit": self.BUFFER_LIMIT,
            "sql_event_limit": self.SQL_EVENT_LIMIT, "git_event_limit": self.GIT_EVENT_LIMIT, "check_event_limit": self.CHECK_EVENT_LIMIT, "file_event_limit": self.FILE_EVENT_LIMIT, "skill_event_limit": self.thread_state.SKILL_LIMIT,
            "history_pending_files": sum(bool(state.get("history_cursor")) for state in self.files.values()),
            "locations": [str(path) for path in list(self.files)[:self.SOURCE_LOCATION_LIMIT]], "listed_file_limit": self.SOURCE_LOCATION_LIMIT,
            "enabled_features": [key for key, enabled in self.features.items() if enabled],
            "checkpoint": {"location": str(self.thread_state.path), "load_health": self.thread_state.load_health, "write_health": self.thread_state.write_health, "retained": len(self.thread_state.entries), "skills": len(self.thread_state.skills), "entry_limit": self.thread_state.LIMIT, "skill_limit": self.thread_state.SKILL_LIMIT, "byte_limit": self.thread_state.BYTE_LIMIT},
        }, "usage": max((state["allowance"] for state in self.files.values() if state.get("allowance")), key=lambda item:item["updated_at"], default=None) if self.features["usage"] else None, "dots": dots, "sqlite": sqlite_summary, "source": "codex", "health": self.health, "started_at": self.started_at, "scope": "recent_file_tail_and_new_records", "track_all": self.track_all, "max_files": self.max_files, "files": len(self.files), "bytes_read": self.read_bytes, "error_backfill_pending": sum(state.get("error_cursor") or 0 for state in self.files.values()), "malformed_lines": self.malformed, "threads": rows, "tools": dict(tools), "nested_tools": dict(nested_tools), "observed_tool_calls": sum(tools.values()), "activity_series": [{"time": key, "calls": value} for key, value in sorted(series.items())[-10080:]], "tool_series": [{"time": key[0], "tool": key[1], "calls": value} for key, value in sorted(tool_series.items())[-20000:]], "mcp_events": mcp_events, "error_events": error_events if self.features["errors"] else [], "file_activity": {"events": file_events[:self.FILE_EVENT_LIMIT], "total": len(file_events), "operations": dict(Counter(event["operation"] for event in file_events))}, "git": {"events": git_events[:self.GIT_EVENT_LIMIT] if self.features["git"] else [], "operations": dict(Counter(event["operation"] for event in git_events)) if self.features["git"] else {}, "total": len(git_events) if self.features["git"] else 0}, "skills": skill_summary, "checks": check_events[:self.CHECK_EVENT_LIMIT] if self.features["checks"] else []}
