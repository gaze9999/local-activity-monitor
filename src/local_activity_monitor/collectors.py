"""Read selected metadata only; never return raw records to HTTP callers."""
from __future__ import annotations
from .thread_state import ThreadState
from .session_checkpoint import SessionCheckpoint
from .agent_messages import agent_messages, parse_message_detail, incoming_message
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

from .codex_metadata import execution_metadata, read_metadata, spawn_metadata
from .codex_schedules import read_schedules
from .codex_plugins import read_plugins
from .payload_detail import bounded_payload, complete_payload, visible_context_record, payload_masking
from .error_records import identifier, tool_error
from .mcp_records import mcp_operations, response_metadata, decoded_output
from .operation_records import file_operations, git_commands, shell_parts, invocations, invocation_expressions, operations, qualified_tool, redact, workflow_operations
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
        result = {"source": "jev", "enabled": enabled, "enabled_at": since, "health": "not_recorded" if enabled else "disabled", "scope": self.SCOPE, "window": window, "summary": {}, "recent": [], "series": [], "reader": {"locations": [str(database)] if database else [], "checked_at": now(), "config_location": str(self.home/"monitoring/jev-monitor.json"), "record_limit": self.RECENT_LIMIT, "series_limit": self.SERIES_LIMIT}}
        if database is None:
            try:
                if (self.home/"monitoring/jev-monitor.json").exists():
                    result["health"] = "invalid_config"
            except OSError as error:
                result["health"] = "unavailable"
                result["reader"]["error_type"] = type(error).__name__
        if not enabled or database is None:
            return result
        hours = WINDOWS[window]
        cutoff = (datetime.now(timezone.utc) - timedelta(hours=hours)).isoformat(timespec="milliseconds").replace("+00:00", "Z") if hours else ""
        try:
            if not database.is_file():
                return result
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
        except (OSError, sqlite3.Error) as error:
            result["health"] = "unavailable"
            if isinstance(error, sqlite3.OperationalError) and str(error).lower().startswith(("no such table:", "no such column:")):
                result["health"] = "unsupported"
            result["reader"]["error_type"] = type(error).__name__
        return result


class CodexCollector:
    """Bounded initial tail, incremental append reads, latest cumulative snapshots."""
    FILE_LIMIT = 5000
    CALL_LIMIT = 50000
    FILE_CALL_LIMIT = 8192
    BUFFER_LIMIT = 8*1024*1024
    READ_LIMIT = 8*1024*1024
    SOURCE_LOCATION_LIMIT = 20
    SCAN_INTERVAL = 15
    SQL_EVENT_LIMIT = 500
    GIT_EVENT_LIMIT = 500
    CHECK_EVENT_LIMIT = 500
    FILE_EVENT_LIMIT = 1000
    SUBAGENT_TAIL_LIMIT = 65536

    def __init__(self, root, max_files=20, tail_bytes=1024*1024):
        self.root, self.max_files, self.tail_bytes = root, max_files, tail_bytes
        self.files = {}
        self.started_at = now()
        self.next_scan = 0
        self.read_bytes = 0
        self.malformed = 0
        self.health = "waiting"
        self.track_all = False
        self.features = {"usage": True, "metadata": True, "git": True, "worktrees": True, "jev_calls": True, "skills": True, "checks": True, "tool_events": True, "mcp": True, "web": True, "files": True, "errors": True, "sqlite": True}
        self.mcp_sources = {}
        self.trimmed_calls = 0
        self.read_cursor = 0
        self.thread_state = ThreadState((root.parent if root.name == "sessions" else root)/"monitoring/thread-state.json")
        self.session_checkpoint = SessionCheckpoint(root)
        self.session_paths, self.related_lifecycle = {}, {}
        self.related_budget = 0
        self.detail_targets, self.detail_indexes = {}, {}
        self.history_sink, self.retired_calls = None, []

    def retire_call(self, state, identity):
        call = state['calls'].pop(identity)
        state.setdefault('retired_ids',set()).add(identity)
        if self.history_sink is not None:
            self.retired_calls.append({'thread_id':state['thread_id'], 'calls':{identity:call}})
        self.trimmed_calls += 1

    def flush_retired(self):
        while self.retired_calls:
            batch = self.retired_calls[:512]
            if not self.history_sink or not self.history_sink(batch):
                self.health = 'partly_unavailable'
                return False
            del self.retired_calls[:len(batch)]
        return True

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
                    info = path.stat()
                    paths.append((info.st_mtime_ns, path, info.st_size))
                except OSError:
                    continue
            ordered = sorted(paths, reverse=True)
            self.session_paths = {}
            for _, path, _ in ordered:
                identities = UUID_IN_NAME.findall(path.name)
                if identities:
                    self.session_paths.setdefault(identities[-1], path)
            active = [path for _, path, _ in ordered]
            self.files = {path: self.files[path] if path in self.files else self.new_file(path) for path in active}
            for _, path, size in ordered:
                self.files[path]["source_size"] = size
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
                state.setdefault('pending_errors',[]).append(state['errors'][-1])
                state["errors"] = sorted(state["errors"], key=lambda event:event["timestamp"] or "", reverse=True)[:50]
        if kind == "session_meta":
            cwd = payload.get("cwd")
            if self.features["worktrees"] and isinstance(cwd, str) and 0 < len(cwd) <= 4096 and not any(ord(char) < 32 for char in cwd):
                state["cwd"] = cwd
            identity = payload.get("id")
            if isinstance(identity, str) and UUID.fullmatch(identity):
                state["thread_id"] = identity
                state["created_at"] = when
            state["activity_type"] = "work" if payload.get("originator") == "codex_work_desktop" else "codex" if payload.get("originator") == "Codex Desktop" else "unknown"
            if payload.get("thread_source") in ("user", "subagent", "guardian_review", "dot", "orbit", "automation", "schedule", "heartbeat"):
                state["trigger"] = payload["thread_source"]
            state["execution"].update(execution_metadata(payload))
            state["execution"].update(spawn_metadata(payload.get('source')))
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
        elif kind == 'response_item' and payload.get('type') == 'message':
            visible = visible_context_record(record)
            if visible is not None:
                for part in visible.get('content', []):
                    if descriptor := incoming_message(part.get('text')):
                        state.setdefault('agent_incoming', []).append(descriptor|{'timestamp':when})
                        state['agent_incoming'] = state['agent_incoming'][-100:]
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
                state['calls'][identity]['agent_messages'] = agent_messages(payload, {'agent_id':state['thread_id']}, calls)
                if len(state["calls"]) > self.FILE_CALL_LIMIT:
                    self.retire_call(state, next(iter(state['calls'])))
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
            elif self.features['errors'] and (failure := tool_error(payload.get('output'))):
                error = failure|{'timestamp':when,'category':'tool','source':'tool_result','severity':'error','call_id':identifier(identity)}
                state['errors'].append(error)
                state.setdefault('pending_errors',[]).append(error)
                state['errors'] = sorted(state['errors'],key=lambda event:event.get('timestamp') or '',reverse=True)[:50]
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

    def refresh(self, pause=None):
        if not self.flush_retired():
            return
        if self.history_sink is not None:
            for path, state in self.files.items():
                if not state.get('dirty_calls') and not state.get('retired_ids') and not state.get('pending_errors'):
                    continue
                changed = {'thread_id':state['thread_id'], 'errors':state.get('pending_errors',[]), 'calls':{identity:state['calls'][identity] for identity in state.get('dirty_calls',()) if identity in state['calls']}}
                if not self.history_sink([changed]):
                    self.health = 'partly_unavailable'
                    return
                self.session_checkpoint.save(path,state)
                state['pending_errors'] = []
        self.scan()
        budget = self.READ_LIMIT
        # Preserve a quarter of each read budget for older metadata even when
        # append-only sessions continuously consume the live allowance.
        reserve = budget//4
        budget -= reserve
        entries = list(self.files.items())
        start_index = self.read_cursor % max(1, len(entries))
        ordered = entries[start_index:]+entries[:start_index]
        for index, (path, state) in enumerate(ordered):
            if budget < 65536 or index >= 1000:
                break
            self.read_cursor = start_index+index+1
            try:
                size = path.stat().st_size
                state["source_size"] = size
                if state["offset"] == size:
                    continue
                if state["offset"] is not None and size < state["offset"]:
                    self.files[path] = state = self.new_file(path)
                if state['offset'] is None and (cached := self.session_checkpoint.load(path,max(0,self.CALL_LIMIT-sum(len(item['calls']) for item in self.files.values())))):
                    state.update(cached)
                with path.open("rb") as stream:
                    if state["offset"] is None:
                        head = stream.readline(65536)
                        self.read_bytes += len(head)
                        state["bytes"] += len(head)
                        budget -= len(head)
                        if head.endswith(b"\n"):
                            self.parse(state, head, 0)
                        state["offset"] = len(head)
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
                    raw = stream.read(max(0, min(budget, self.tail_bytes, 256*1024 if pause else self.tail_bytes)))
                    state["offset"] += len(raw)
                    state["bytes"] += len(raw)
                    budget -= len(raw)
                    self.read_bytes += len(raw)
                line_offset = state["offset"]-len(raw)-len(state["buffer"])
                data = state["buffer"] + raw
                lines = data.split(b"\n")
                state["buffer"] = lines.pop()
                if state["discard"] and lines:
                    skipped = lines.pop(0)
                    line_offset += len(skipped)+1
                    if state["history_cursor"] is not None:
                        state["history_cursor"] += len(skipped)+1
                    if state.get("error_cursor") is not None:
                        state["error_cursor"] += len(skipped)+1
                    state["discard"] = False
                for line in lines:
                    self.parse(state, line, line_offset)
                    line_offset += len(line)+1
                if len(state["buffer"]) > 1024*1024:
                    state["buffer"] = b""
                    state["discard"] = True
                state["partial_history"] = state["offset"] < size
                if not self.flush_retired():
                    return
                changed = {'thread_id':state['thread_id'], 'errors':state.get('pending_errors',[]), 'calls':{identity:state['calls'][identity] for identity in state.get('dirty_calls',()) if identity in state['calls']}}
                if self.history_sink is not None and not self.history_sink([changed]):
                    self.health = 'partly_unavailable'
                    return
                if self.history_sink is not None:
                    self.session_checkpoint.save(path, state)
                    state['pending_errors'] = []
                if raw and pause and pause():
                    return
            except OSError:
                self.health = "partly_unavailable"
        else:
            self.read_cursor = start_index+1
        budget += reserve
        error_reserve = min(budget, self.READ_LIMIT//8) if self.features["errors"] and any(state.get("error_cursor") for state in self.files.values()) else 0
        budget -= error_reserve
        for path, state in ordered:
            # One bounded chunk per file prevents one large session starving
            # every other pending lifecycle cursor.
            if budget > 0 and state["history_cursor"]:
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
                if raw and pause and pause():
                    return
        budget += error_reserve
        if self.features["errors"]:
            cutoff = datetime.now(timezone.utc)-timedelta(hours=24)
            for path, state in ordered:
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
                positioned = []
                line_offset = begin+len(first)+1 if begin else 0
                for line in lines:
                    positioned.append((line_offset, line))
                    line_offset += len(line)+1
                for line_offset, line in reversed(positioned):
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
                            self.parse(state, line, line_offset)
                        elif kind == "response_item" and payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                            failure = tool_error(payload.get("output"))
                            if failure:
                                state["errors"].append({"timestamp": record["timestamp"], "source": "tool_result", "category": "tool", "severity": "error", "call_id": identifier(payload.get("call_id")), "record_offset": line_offset, "record_hash": hashlib.sha256(line).hexdigest(), **failure})
                                state["errors"] = sorted(state["errors"], key=lambda event:event["timestamp"] or "", reverse=True)[:50]
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
                if raw and pause and pause():
                    return
        retained = sum(len(state["calls"]) for state in self.files.values())
        if retained > self.CALL_LIMIT:
            oldest = sorted((call.get("timestamp") or "", str(path), identity, state) for path, state in self.files.items() for identity, call in state["calls"].items())
            for _, _, identity, state in oldest[:retained-self.CALL_LIMIT]:
                self.retire_call(state, identity)
        if not self.flush_retired():
            return
        if self.history_sink is not None:
            for path,state in self.files.items():
                if state.get('retired_ids'):
                    self.session_checkpoint.save(path,state)
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
        self.related_budget = max(0, budget)
        self.backfill_detail_index()

    def select_detail_targets(self, events):
        """Index only retained observed calls, never persist source content or offsets."""
        targets = {}
        for event in events[:2500]:
            identity, call = event.get("thread_id"), event.get("call_id")
            if isinstance(identity, str) and UUID.fullmatch(identity) and isinstance(call, str) and NAME.fullmatch(call):
                targets.setdefault(identity, set()).add(call)
        self.detail_targets = targets
        self.detail_indexes = {identity: index for identity, index in self.detail_indexes.items() if identity in targets}

    def backfill_detail_index(self):
        for identity, targets in self.detail_targets.items():
            path = self.session_paths.get(identity)
            if not path or path.is_symlink():
                continue
            try:
                stat = path.stat()
                index = self.detail_indexes.setdefault(identity, {"cursor": stat.st_size, "size": stat.st_size, "modified": stat.st_mtime_ns, "positions": {}, "targets": set()})
                if stat.st_size < index["size"] or stat.st_size == index["size"] and stat.st_mtime_ns != index["modified"]:
                    index.update(cursor=stat.st_size, positions={})
                added = targets-index["targets"]
                index["size"] = stat.st_size
                index["modified"] = stat.st_mtime_ns
                index["targets"] = set(targets)
                index["positions"] = {call: positions for call, positions in index["positions"].items() if call in targets}
                for state in self.files.values():
                    if state["thread_id"] == identity:
                        for call in targets & state["calls"].keys():
                            positions = {key: state["calls"][call][key] for key in ("request_offset", "response_offset") if key in state["calls"][call]}
                            index["positions"].setdefault(call, {}).update(positions)
                # Recent requests already have a position, even while awaiting output.
                # Restart only for newly retained calls whose older request is unknown.
                if any("request_offset" not in index["positions"].get(call, {}) for call in added):
                    index["cursor"] = stat.st_size
                if all("request_offset" in index["positions"].get(call, {}) and "response_offset" in index["positions"].get(call, {}) for call in targets):
                    continue
                end = index["cursor"]
                if end <= 0 or self.related_budget <= 0:
                    continue
                start = max(0, end-min(self.related_budget, self.READ_LIMIT))
                with path.open("rb") as stream:
                    stream.seek(start)
                    body = stream.read(end-start)
                self.related_budget -= len(body)
                self.read_bytes += len(body)
                lines = body.split(b"\n")
                first = lines.pop(0) if start else b""
                line_offset = start+len(first)+1 if start else 0
                index["cursor"] = line_offset if line_offset < end else start
                needles = [call.encode("utf-8") for call in targets]
                for raw in lines:
                    if len(raw) <= 1024*1024 and any(needle in raw for needle in needles):
                        try:
                            record = json.loads(raw)
                            payload = record.get("payload") if isinstance(record, dict) else None
                            call = payload.get("call_id") or payload.get("id") if isinstance(payload, dict) else None
                            if record.get("type") == "response_item" and call in targets:
                                kind = payload.get("type")
                                if kind in ("function_call", "custom_tool_call", "function_call_output", "custom_tool_call_output"):
                                    key = "request_offset" if kind in ("function_call", "custom_tool_call") else "response_offset"
                                    index["positions"].setdefault(call, {}).setdefault(key, line_offset)
                        except (ValueError, AttributeError, RecursionError):
                            pass
                    line_offset += len(raw)+1
            except OSError:
                continue

    def detail_position(self, identity, call, observed=None):
        state_call = observed or {}
        indexed = self.detail_indexes.get(identity, {}).get("positions", {}).get(call, {})
        return indexed | {key: state_call[key] for key in ("request_offset", "response_offset") if key in state_call}

    def detail_status(self, identity, call):
        path = self.session_paths.get(identity)
        if path is None or not path.is_file() or path.is_symlink():
            return "unavailable"
        index = self.detail_indexes.get(identity)
        return "pending" if identity in self.detail_targets and (index is None or index["cursor"] > 0) else "unavailable"

    def subagent_lifecycle(self, metadata, source_info):
        """Select lifecycle events from related session tails, cached by file signature."""
        selected = {identity for identity, entry in metadata.items() if entry.get("execution", {}).get("parent_thread_id")}
        self.related_lifecycle = {identity: value for identity, value in self.related_lifecycle.items() if identity in selected}
        report = {"name": "subagent_lifecycle", "location": str(self.root), "health": "ok", "checked_at": now(), "fields": ["thread_id", "task_started", "task_complete", "timestamp"], "file_limit": self.FILE_LIMIT, "tail_bytes": self.SUBAGENT_TAIL_LIMIT, "rows_read": 0, "bytes_read": 0, "missing_files": 0, "pending_files": 0}
        loaded = {state["thread_id"] for state in self.files.values()}
        for identity in sorted(selected):
            if identity in loaded:
                continue
            path = self.session_paths.get(identity)
            if path is None:
                report["missing_files"] += 1
                continue
            try:
                if path.is_symlink():
                    report["health"] = "partly_unavailable"
                    continue
                stat = path.stat()
                signature = (stat.st_size, stat.st_mtime_ns)
                cached = self.related_lifecycle.get(identity)
                if cached and cached["signature"] == signature:
                    metadata[identity].update(cached["metadata"])
                    continue
                length = min(stat.st_size, self.SUBAGENT_TAIL_LIMIT)
                if self.related_budget < length:
                    report["pending_files"] += 1
                    continue
                with path.open("rb") as stream:
                    offset = max(0, stat.st_size-length)
                    stream.seek(offset)
                    body = stream.read(length)
                self.related_budget -= len(body)
                self.read_bytes += len(body)
                report["bytes_read"] += len(body)
                if offset:
                    body = body.partition(b"\n")[2]
                status = {}
                for line in body.splitlines():
                    if b'"task_started"' not in line and b'"task_complete"' not in line:
                        continue
                    try:
                        record = json.loads(line)
                        payload = record.get("payload") if isinstance(record, dict) else None
                        if record.get("type") != "event_msg" or not isinstance(payload, dict) or payload.get("type") not in ("task_started", "task_complete"):
                            continue
                        when = timestamp(record.get("timestamp"))
                        if when and when >= status.get("status_updated_at", ""):
                            status = {"status": "running" if payload["type"] == "task_started" else "completed", "status_updated_at": when, "status_source": "session_lifecycle"}
                            report["rows_read"] += 1
                    except (ValueError, AttributeError, RecursionError):
                        continue
                self.related_lifecycle[identity] = {"signature": signature, "metadata": status}
                metadata[identity].update(status)
            except OSError:
                report["health"] = "partly_unavailable"
        source_info[str(self.root)+"#subagent_lifecycle"] = report

    def parse(self, state, raw, offset=None):
        if not raw or len(raw) > 1024*1024:
            return
        try:
            record = json.loads(raw)
            if isinstance(record, dict):
                self.consume(state, record)
                payload = record.get("payload")
                if offset is not None and record.get('type') == 'event_msg' and isinstance(payload, dict):
                    for event in reversed(state['errors']):
                        if event.get('source') == 'session' and event.get('timestamp') == timestamp(record.get('timestamp')) and event.get('reason') == payload.get('type'):
                            event.update(record_offset=offset, record_hash=hashlib.sha256(raw.rstrip(b'\n')).hexdigest())
                            break
                if offset is not None and record.get("type") == "response_item" and isinstance(payload, dict):
                    identity = payload.get("call_id") or payload.get("id")
                    call = state["calls"].get(identity)
                    kind = payload.get("type")
                    if not call and kind in ('function_call_output','custom_tool_call_output'):
                        for event in reversed(state['errors']):
                            if event.get('source')=='tool_result' and event.get('call_id')==identity and event.get('timestamp')==timestamp(record.get('timestamp')):
                                event.update(record_offset=offset,record_hash=hashlib.sha256(raw.rstrip(b'\n')).hexdigest())
                                break
                    if call and kind in ("function_call", "custom_tool_call", "function_call_output", "custom_tool_call_output"):
                        state.setdefault('dirty_calls',set()).add(identity)
                        call["request_offset" if kind in ("function_call", "custom_tool_call") else "response_offset"] = offset
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
        if not self.features.get('git', True):
            return None
        return self.command_detail(thread_id, call_id, operation, 'git', full)

    def check_detail(self, thread_id, call_id, operation, full=False):
        if not self.features.get('checks', True):
            return None
        return self.command_detail(thread_id, call_id, operation, 'checks', full)

    def command_detail(self, thread_id, call_id, operation, category, full=False):
        """Return observed Git/check commands and their containing response on demand."""
        result = {'commands': [], 'request': None, 'response': None, 'output': None,
                  'output_scope': None, 'result_scope': None, 'truncated': False}
        for path, state in self.detail_states(thread_id):
            call = state['calls'].get(call_id)
            retained = getattr(self, 'detail_targets', {}).get(thread_id, set())
            if not call and call_id not in retained:
                continue
            if call and not any(item['operation'] == operation for item in call.get(category, [])):
                continue
            if path.is_symlink():
                continue
            commands, output, request, outer = [], None, None, True
            try:
                for raw in self.detail_lines(path, self.detail_position(thread_id, call_id, call)):
                    try:
                        record = json.loads(raw)
                        payload = record.get('payload') if isinstance(record, dict) else None
                        if record.get('type') != 'response_item' or not isinstance(payload, dict) or (payload.get('call_id') or payload.get('id')) != call_id:
                            continue
                        if payload.get('type') in ('function_call', 'custom_tool_call'):
                            calls = invocations(payload)
                            selected, parts = [], []
                            for tool, args, nested in calls:
                                if tool not in ('exec_command', 'functions.exec_command') or not isinstance(args, dict) or not isinstance(args.get('cmd'), str):
                                    continue
                                for part in shell_parts(args['cmd']):
                                    parts.append(part)
                                    if category == 'git':
                                        matches = operation in git_commands(part)
                                    else:
                                        check_payload = {'name': 'exec_command', 'arguments': {'cmd': part}}
                                        _, checks = workflow_operations(check_payload, include_skills=False)
                                        matches = any(item['operation'] == operation for item in checks)
                                    if matches:
                                        selected.append(part.strip())
                            if selected:
                                commands = selected
                                request = payload.get('arguments', payload.get('input'))
                                outer = len(calls) != 1 or any(item[2] for item in calls) or len(parts) != 1
                        elif payload.get('type') in ('function_call_output', 'custom_tool_call_output'):
                            output = payload.get('output')
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
                if commands:
                    present = complete_payload if full else bounded_payload
                    sent, returned = present(request), present(output)
                    command_text = '\n'.join(commands)
                    result.update(commands=[present(command_text)['value']],
                                  request=sent['value'], response=returned['value'], output=returned['value'],
                                  output_scope='outer_scope' if outer else 'git_command' if category == 'git' else 'check_command',
                                  result_scope='containing_tool_call' if outer else 'git_command' if category == 'git' else 'check_command',
                                  content_status='available',
                                  truncated=sent['truncated'] or returned['truncated'] or not full and len(command_text)>32768)
                    return result
            except (OSError, ValueError, RuntimeError):
                continue
        return result | {'content_status': self.detail_status(thread_id, call_id)}

    def sql_detail(self, event, mask=True, full=False):
        for path, state in self.detail_states(event.get("thread_id")):
            try:
                if path.is_symlink():
                    continue
                positions = self.detail_position(event.get("thread_id"), event.get("call_id"), state["calls"].get(event.get("call_id")))
                result, output = None, None
                for raw in self.detail_lines(path, positions):
                    try:
                        record = json.loads(raw)
                        payload = record.get("payload") if isinstance(record, dict) else None
                        if not isinstance(payload, dict) or record.get("type") != "response_item" or (payload.get("call_id") or payload.get("id")) != event.get("call_id"):
                            continue
                        if payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                            output = payload.get("output")
                            continue
                        if payload.get("type") not in ("function_call", "custom_tool_call"):
                            continue
                        calls = sqlite_operations(invocations(payload), include_sql=True)
                        index = event.get("index", -1)
                        if 0 <= index < len(calls) and calls[index]["statement"] == event["statement"]:
                            result = sql_content(calls[index].get("sql"), mask, full)
                    except (ValueError, AttributeError, RecursionError):
                        continue
                if result is not None:
                    returned = (complete_payload if full else bounded_payload)(output)
                    return result | {"content_status": "available" if result["sql"] is not None else "unavailable",
                                     "response": returned["value"], "response_scope": "containing_tool_call",
                                     "truncated": result["truncated"] or returned["truncated"]}
            except OSError:
                continue
        return sql_content(None) | {"content_status": self.detail_status(event.get("thread_id"), event.get("call_id"))}

    def detail_states(self, identity):
        loaded = [(path, state) for path, state in self.files.items() if state["thread_id"] == identity]
        if loaded:
            return loaded
        path = self.session_paths.get(identity)
        return [(path, {"calls": {}})] if path else []

    def detail_lines(self, path, call):
        """Read observed record positions and a bounded legacy tail, never scan a source."""
        with path.open("rb") as stream:
            seen, budget = set(), self.READ_LIMIT
            for field in ("request_offset", "response_offset", "record_offset"):
                offset = call.get(field)
                if type(offset) is not int or offset < 0 or offset in seen or budget <= 0:
                    continue
                seen.add(offset)
                stream.seek(offset)
                raw = stream.readline(min(1024*1024+1, budget))
                budget -= len(raw)
                if raw.endswith(b"\n") and len(raw) <= 1024*1024:
                    yield raw
            stream.seek(0, 2)
            observed_size = self.files.get(path, {}).get('offset')
            if observed_size is None:
                observed_size = next((index.get('size') for identity, index in self.detail_indexes.items() if self.session_paths.get(identity) == path), None)
            if observed_size == stream.tell() and (type(call.get('record_offset')) is int or all(type(call.get(field)) is int for field in ('request_offset', 'response_offset'))):
                return
            offset = max(0, stream.tell()-min(self.tail_bytes, budget))
            stream.seek(offset)
            if offset:
                stream.readline()
            for raw in stream.read(min(self.tail_bytes, budget)).splitlines():
                yield raw

    def jev_detail(self, thread_id, call_id, index, full=False):
        if not self.features.get('jev_calls', True) or type(index) is not int or index < 0:
            return None
        request, output, nested, operation, isolated = None, None, False, None, True
        for path, state in self.detail_states(thread_id):
            call = state['calls'].get(call_id)
            if path.is_symlink() or not call or index >= len(call.get('jev', [])):
                continue
            try:
                for raw in self.detail_lines(path, self.detail_position(thread_id, call_id, call)):
                    try:
                        record = json.loads(raw)
                        payload = record.get('payload') if isinstance(record, dict) else None
                        if record.get('type') != 'response_item' or not isinstance(payload, dict) or (payload.get('call_id') or payload.get('id')) != call_id:
                            continue
                        if payload.get('type') in ('function_call', 'custom_tool_call'):
                            _, calls = operations(payload)
                            if index < len(calls):
                                request, nested, operation = calls[index]['arguments'], calls[index]['nested'], calls[index]['operation']
                                isolated = not nested or len(invocations(payload)) == 1
                        elif payload.get('type') in ('function_call_output', 'custom_tool_call_output'):
                            output = payload.get('output')
                    except (ValueError, AttributeError, RecursionError):
                        continue
            except OSError:
                continue
        if full and not operation:
            return None
        present = complete_payload if full else bounded_payload
        sent, returned = present(request if operation else None), present(output if operation and isolated else None)
        return {'operation': operation, 'request': sent['value'], 'response': returned['value'],
                'response_scope': 'containing_tool_call' if nested else 'jev_tool_call',
                'truncated': sent['truncated'] or returned['truncated']}

    def context_detail(self, thread_id, call_id=None):
        for path,state in self.detail_states(thread_id):
            if path.is_symlink() or state.get('thread_id')!=thread_id:
                continue
            if call_id and call_id not in state['calls']:
                return None
            try:
                call = state['calls'].get(call_id,{})
                position = self.detail_position(thread_id,call_id,call) if call_id else {}
                if call_id and type(position.get('request_offset')) is not int:
                    return {'text':[],'scope':'preceding_visible_messages','truncated':False,'context_state':'pending','masked':payload_masking.get()}
                end = position['request_offset'] if call_id else path.stat().st_size
                start = max(0,end-1024*1024)
                with path.open('rb') as stream:
                    stream.seek(start)
                    raw = stream.read(end-start)
                lines = raw.split(b'\n')
                record_offset = start
                if start:
                    record_offset += len(lines[0])+1
                    lines = lines[1:]
                messages = []
                for line in lines:
                    offset = record_offset
                    record_offset += len(line)+1
                    if not line or len(line)>1024*1024:
                        continue
                    try:
                        record = json.loads(line)
                        value = visible_context_record(record)
                        if value is not None:
                            messages.append({'record_offset':offset,'timestamp':timestamp(record.get('timestamp')),**value})
                    except (ValueError,TypeError,AttributeError,RecursionError):
                        continue
                return {'text':complete_payload(messages[-32:])['value'],'scope':'preceding_visible_messages' if call_id else 'latest_visible_messages','truncated':start>0 or len(messages)>32,'context_state':'recorded' if messages else 'not_recorded','masked':payload_masking.get()}
            except OSError:
                return None
        return None

    def agent_message_detail(self, thread_id, call_id, index=0):
        content = self.tool_detail(thread_id,call_id,full=True)
        if content is None:
            return None
        payload = {'type':'function_call','name':content['tool'],'arguments':content['request']}
        selected = parse_message_detail(payload,index,mask=payload_masking.get())
        if selected is None:
            return None
        return selected|{'response':content['response'],'response_scope':'containing_tool_call','text':self.context_detail(thread_id,call_id),'content_status':content['content_status'],'truncated':content['truncated']}

    def tool_detail(self, thread_id, call_id, full=False):
        if not self.features.get('tool_events', True):
            return None
        for path, state in self.detail_states(thread_id):
            call = state['calls'].get(call_id)
            if path.is_symlink() or not call and call_id not in self.detail_targets.get(thread_id, set()):
                continue
            request, output, found, tool = None, None, False, call.get('tool') if call else None
            try:
                for raw in self.detail_lines(path, self.detail_position(thread_id, call_id, call)):
                    try:
                        record = json.loads(raw)
                        payload = record.get('payload') if isinstance(record, dict) else None
                        if record.get('type') != 'response_item' or not isinstance(payload, dict) or (payload.get('call_id') or payload.get('id')) != call_id:
                            continue
                        if payload.get('type') in ('function_call', 'custom_tool_call') and (tool is None or name(qualified_tool(payload)) == tool):
                            tool = name(qualified_tool(payload))
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
                return {'tool': tool, 'request': sent['value'], 'response': returned['value'],
                        'response_scope': 'tool_call', 'content_status': 'available',
                        'truncated': sent['truncated'] or returned['truncated']}
            except OSError:
                return None
        return None

    def error_detail(self, event, full=False):
        """Read the exact observed session failure; no prompt/message records qualify."""
        if not self.features.get('errors', True) or event.get('source') not in ('session', 'tool_result'):
            return None
        thread_id, call_id = event.get('thread_id'), event.get('call_id')
        for path, state in self.detail_states(thread_id):
            if path.is_symlink():
                continue
            call = state['calls'].get(call_id) if call_id else None
            try:
                positions = self.detail_position(thread_id, call_id, call) if call_id else {}
                if type(event.get("record_offset")) is int:
                    positions["record_offset"] = event["record_offset"]
                for raw in self.detail_lines(path, positions):
                    try:
                        record = json.loads(raw)
                        payload = record.get('payload') if isinstance(record, dict) else None
                        if event.get("record_hash") and hashlib.sha256(raw.rstrip(b"\n")).hexdigest() != event["record_hash"]:
                            continue
                        if not isinstance(payload, dict) or timestamp(record.get('timestamp')) != timestamp(event.get('timestamp')):
                            continue
                        if event['source'] == 'tool_result':
                            if record.get('type') != 'response_item' or payload.get('type') not in ('function_call_output', 'custom_tool_call_output') or (payload.get('call_id') or payload.get('id')) != call_id:
                                continue
                            value = payload.get('output')
                            if not tool_error(value):
                                continue
                        else:
                            kind = payload.get('type')
                            if record.get('type') != 'event_msg' or kind != event.get('reason') or kind not in ('error', 'turn_aborted', 'turn_failed') and not kind.endswith('_failed'):
                                continue
                            value = {key: payload[key] for key in ('type', 'message', 'error', 'reason', 'error_code') if key in payload}
                        presented = (complete_payload if full else bounded_payload)(value)
                        return {'text': presented['value'], 'truncated': presented['truncated'], 'content_status': 'available', 'source': event['source']}
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
            except OSError:
                continue
        return {'text': None, 'truncated': False, 'content_status': self.detail_status(thread_id, call_id) if call_id else 'unavailable', 'source': event['source']}

    def mcp_detail(self, thread_id, call_id, index, full=False, observed_event=None):
        for path, state in self.detail_states(thread_id):
            call = state["calls"].get(call_id)
            events = call.get("mcp", []) if call else []
            if path.is_symlink() or not 0 <= index < len(events) and observed_event is None:
                continue
            event = events[index] if 0 <= index < len(events) else observed_event
            if self.mcp_sources.get(event["server"]) is False:
                return None
            request, output, observed, request_source = None, None, False, None
            try:
                for raw in self.detail_lines(path, self.detail_position(thread_id, call_id, call)):
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
                            if request is None and event["nested"]:
                                expressions = [(tool, expression) for tool, expression in invocation_expressions(payload) if mcp_operations([(tool, None, False)])]
                                if index < len(expressions) and expressions[index][0] == eligible[index][0]:
                                    request, request_source = expressions[index][1], "recorded_expression"
                        elif payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                            output = payload.get("output")
                    except (ValueError, TypeError, AttributeError, RecursionError):
                        continue
                present = complete_payload if full else bounded_payload
                sent, returned = present(request), present(output)
                return {"request": sent["value"] if observed else None, "response": returned["value"] if observed else None,
                        "response_scope": "containing_tool_call" if event["nested"] else "mcp_tool_call",
                        "request_source": request_source,
                        "content_status": "available" if observed else self.detail_status(thread_id, call_id),
                        "truncated": sent["truncated"] or returned["truncated"]}
            except OSError:
                continue
        return None

    def snapshot_windows(self, pause=None):
        metadata_cache = {}
        reference = datetime.now(timezone.utc)
        snapshots = {}
        for window in WINDOWS:
            snapshots[window] = self.snapshot(window, reference, metadata_cache)
            if pause and pause():
                break
        return snapshots

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
            target.setdefault('agent_incoming', []).extend(state.get('agent_incoming', []))
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
            metadata, dots, metadata_sources, schedules, plugins = metadata_cache["metadata"]
        else:
            self.project_details = {}
            metadata = read_metadata(self.root.parent, threads, dots, metadata_sources, self.project_details, include_workdirs=self.features["worktrees"], select_recent=self.health == "waiting" and not self.files) if self.features["metadata"] else {}
            if self.features["metadata"]:
                self.subagent_lifecycle(metadata, metadata_sources)
            schedules = read_schedules(self.root.parent, metadata_sources, metadata) if self.features["metadata"] else {"items": [], "health": "disabled"}
            plugins = read_plugins(self.root.parent, metadata_sources) if self.features["metadata"] else {"items": [], "health": "disabled"}
            if metadata_cache is not None:
                metadata_cache["metadata"] = metadata, dots, metadata_sources, schedules, plugins
        dots = dict(dots)
        self.thread_workdirs = {identity: entry["_cwd"] for identity, entry in metadata.items() if entry.get("_cwd")} if self.features["worktrees"] else {}
        if self.features["worktrees"]:
            for state in sorted(self.files.values(), key=lambda item: item.get("updated_at") or "", reverse=True):
                if state.get("cwd"):
                    self.thread_workdirs.setdefault(state["thread_id"], state["cwd"])
        dot_events = dots.pop("_retained_events", dots.get("events"))
        if dot_events is not None:
            scoped_dots = [event for event in dot_events if contains(event, boundary)]
            dots.update(events=sorted(scoped_dots, key=lambda item:item.get("timestamp") or "", reverse=True)[:500], total=len(scoped_dots))
        dots.update(window=window, activity_items_scope="latest_loaded_snapshot")
        dots['activity'] = [event for event in dots.get('activity',[]) if contains(event,boundary)]
        pending_status = {state["thread_id"] for state in self.files.values() if state.get("history_cursor") or state["offset"] is None or state["offset"] < state.get("source_size", 0)}
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
            row['status_updated_at'] = thread.get('task_time') or entry.get('status_updated_at') or (entry.get('updated_at') if entry.get('status') else cached.get('task_time'))
            row['status_source'] = ('cached_lifecycle' if thread.get('status_cached') else 'session_lifecycle') if thread.get('task_time') else entry.get('status_source') or ('catalog_status' if entry.get('status') else 'cached_lifecycle' if cached else None)
            row["status_backfill_pending"] = thread["thread_id"] in pending_status
            row["context_updated_at"] = thread.get("context_time") or None
            environment = entry.get("environment", "unknown")
            if environment == "unknown" and not thread.get("metadata_only"):
                environment = "local"
            row.update({"thread_name": entry.get("thread_name"), "environment": environment, "project_id": entry.get("project_id"), "project_name": entry.get("project_name"), "project_scope": entry.get("project_scope", "unknown"), "has_schedule": entry.get("has_schedule", False)})
            row['host_id'] = entry.get('_host_id')
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
            communication = [event|{'thread_id':thread['thread_id']} for event in thread.get('agent_incoming',[]) if contains(event,boundary)]
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
                communication.extend(context|event for event in call.get('agent_messages',[]))
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
            row['agent_messages'] = sorted(communication,key=lambda event:event.get('timestamp') or '',reverse=True)[:100]
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
        for event in error_events:
            identity = [event.get(key) for key in ('source', 'thread_id', 'call_id', 'timestamp', 'reason', 'code', 'record_hash')]
            event['content_id'] = hashlib.sha256(json.dumps(identity).encode()).hexdigest()
        return {"projects": list(projects.values()), "schedules": schedules, "plugins": plugins, "activity_scope": {"window": window, "timestamp": "call_started_at", "unknown_timestamp": "all_only", "latest_state": ["tokens", "model", "status", "usage", "schedules", "worktrees"]}, "tool_statistics": statistics, "metadata_sources": list(metadata_sources.values()), "read_state": {
            "file_limit": None, "file_count": len(self.files), "pending_files": sum(state["offset"] is None or state["offset"] < state.get("source_size", 0) for state in self.files.values()), "read_mode": "incremental_all_sources",
            "read_limit": self.READ_LIMIT, "tail_bytes": self.tail_bytes, "scan_seconds": self.SCAN_INTERVAL,
            "call_limit": self.CALL_LIMIT, "buffer_limit": self.BUFFER_LIMIT,
            "sql_event_limit": self.SQL_EVENT_LIMIT, "git_event_limit": self.GIT_EVENT_LIMIT, "check_event_limit": self.CHECK_EVENT_LIMIT, "file_event_limit": self.FILE_EVENT_LIMIT, "skill_event_limit": self.thread_state.SKILL_LIMIT,
            "history_pending_files": sum(bool(state.get("history_cursor")) for state in self.files.values()),
            "history_pending_bytes": sum(state.get("history_cursor") or 0 for state in self.files.values()),
            "error_pending_files": sum(bool(state.get("error_cursor")) for state in self.files.values()) if self.features["errors"] else 0,
            "error_pending_bytes": sum(state.get("error_cursor") or 0 for state in self.files.values()) if self.features["errors"] else 0,
            "initial_pending_files": sum(state["offset"] is None for state in self.files.values()),
            "detail_index_pending_bytes": sum(index["cursor"] for index in self.detail_indexes.values() if not all("request_offset" in index["positions"].get(call, {}) and "response_offset" in index["positions"].get(call, {}) for call in index["targets"])),
            "detail_index_calls": sum(len(index["positions"]) for index in self.detail_indexes.values()),
            "locations": [str(path) for path in list(self.files)[:self.SOURCE_LOCATION_LIMIT]], "listed_file_limit": self.SOURCE_LOCATION_LIMIT,
            "enabled_features": [key for key, enabled in self.features.items() if enabled],
            "checkpoint": {"location": str(self.thread_state.path), "load_health": self.thread_state.load_health, "write_health": self.thread_state.write_health, "load_checked_at": self.thread_state.load_checked_at, "write_checked_at": self.thread_state.write_checked_at, "retained": len(self.thread_state.entries), "skills": len(self.thread_state.skills), "entry_limit": self.thread_state.LIMIT, "skill_limit": self.thread_state.SKILL_LIMIT, "byte_limit": self.thread_state.BYTE_LIMIT},
        }, "usage": max((state["allowance"] for state in self.files.values() if state.get("allowance")), key=lambda item:item["updated_at"], default=None) if self.features["usage"] else None, "dots": dots, "sqlite": sqlite_summary, "source": "codex", "health": self.health, "started_at": self.started_at, "scope": "incremental_all_sources", "track_all": self.track_all, "max_files": self.max_files, "files": len(self.files), "bytes_read": self.read_bytes, "error_backfill_pending": sum(state.get("error_cursor") or 0 for state in self.files.values()), "malformed_lines": self.malformed, "threads": rows, "tools": dict(tools), "nested_tools": dict(nested_tools), "observed_tool_calls": sum(tools.values()), "activity_series": [{"time": key, "calls": value} for key, value in sorted(series.items())[-10080:]], "tool_series": [{"time": key[0], "tool": key[1], "calls": value} for key, value in sorted(tool_series.items())[-20000:]], "mcp_events": mcp_events, "error_events": error_events if self.features["errors"] else [], "file_activity": {"events": file_events[:self.FILE_EVENT_LIMIT], "total": len(file_events), "operations": dict(Counter(event["operation"] for event in file_events))}, "git": {"events": git_events[:self.GIT_EVENT_LIMIT] if self.features["git"] else [], "operations": dict(Counter(event["operation"] for event in git_events)) if self.features["git"] else {}, "total": len(git_events) if self.features["git"] else 0}, "skills": skill_summary, "checks": check_events[:self.CHECK_EVENT_LIMIT] if self.features["checks"] else []}
