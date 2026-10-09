"""Serve a local metadata dashboard using only the Python standard library."""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime
import base64
import copy
import gzip
import hashlib
from http.client import HTTPConnection, HTTPException
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import sqlite3
import json
import os
import re
import sys
from pathlib import Path
from socketserver import TCPServer
import tempfile
import threading
import time
import uuid
from urllib.parse import parse_qs, urlsplit, urlencode
import webbrowser

from .collectors import CodexCollector, JevCollector, WINDOWS, monitor_config, name as tool_name, now
from .activity_windows import cutoff, contains
from .payload_detail import mask_payloads, payload_masking
from .project_instructions import instructions, observed_file_metadata
from .codex_connection import connection_status
from .idle_activity import IdleActivity
from .mcp_source_files import documents as source_documents, read_document, write_document
from .mcp_records import CATEGORIES, EVENT_LIMIT as MCP_EVENT_LIMIT, TOOL_LIMIT as MCP_TOOL_LIMIT, SOURCE, category, discover_sources, summarize
from .monitor_state import MonitorState
from .worktree_info import WorktreeCollector
from .error_records import DiagnosticCollector, error_summary, FAILURES
from .api_records import api_summary
from .error_history import ErrorHistory, error_identity
from .payload_detail import paged_content
from .activity_history import ActivityHistory, sql_identity, mcp_identity, merge_event
from .codex_account import CodexAccountSource
from .ui_assets import load_ui_assets
from .frontend_assets import load_frontend_assets
from .process_lifecycle import cli_lifetime


def data_root():
    if os.name == "nt":
        value = Path(os.environ.get("LOCALAPPDATA", ""))
        return (value if value.is_absolute() else Path.home()/"AppData/Local") / "local-activity-monitor"
    if os.sys.platform == "darwin":
        return Path.home()/"Library/Application Support/local-activity-monitor"
    value = Path(os.environ.get("XDG_DATA_HOME", ""))
    return (value if value.is_absolute() else Path.home()/".local/share") / "local-activity-monitor"


def configure(home, enabled, database=None):
    path = home / "monitoring/jev-monitor.json"
    before = path.read_bytes() if path.exists() else None
    old_enabled, old_database, since = monitor_config(home)
    if before is not None and old_database is None:
        raise ValueError("Existing monitor config is invalid; inspect it before replacement")
    database = (database or old_database or data_root()/"state/jev.sqlite3").expanduser()
    if not database.is_absolute():
        raise ValueError("Database path must be absolute")
    value = {"version": 1, "enabled": enabled, "database": str(database), "enabled_at": since if old_database == database and since else now()}
    after = (json.dumps(value, indent=2)+"\n").encode("utf-8")
    if before is not None and json.loads(before) == value:
        return "unchanged"
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if before is not None:
        backup = path.with_name(path.name+"."+uuid.uuid4().hex[:8]+".bak")
        with backup.open("xb") as stream:
            stream.write(before)
        if os.name != "nt":
            backup.chmod(0o600)
    descriptor, temporary = tempfile.mkstemp(dir=path.parent, prefix=".monitor-")
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(after)
            stream.flush()
            os.fsync(stream.fileno())
        current = path.read_bytes() if path.exists() else None
        if current != before:
            raise ValueError("Monitor config changed during update")
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    return "enabled" if enabled else "disabled"


def home_id(home):
    return hashlib.sha256(os.path.normcase(str(home.resolve())).encode("utf-8")).hexdigest()[:24]


def existing_instance(port, home):
    if not port:
        return None
    url = f"http://127.0.0.1:{port}/"
    try:
        connection = HTTPConnection("127.0.0.1", port, timeout=2)
        try:
            # Read the complete response before closing, as browsers do with HTTP/1.1.
            connection.request("GET", "/api/instance")
            response = connection.getresponse()
            if response.status != 200:
                return None
            value = json.loads(response.read(4097))
        finally:
            connection.close()
        return url if isinstance(value, dict) and value.get("application")=="local-activity-monitor" and value.get("home_id")==home_id(home) else None
    except (OSError, ValueError, HTTPException):
        return None


class Dashboard:
    def __init__(self, home, codex=False, max_files=20, interval=10):
        self.home, self.max_files = home, max_files
        self.jev = JevCollector(home)
        self.codex = CodexCollector(home/"sessions", max_files) if codex else None
        self.account = CodexAccountSource(home)
        self.interval = interval
        self.idle_minutes = 5
        self.activity = IdleActivity(home)
        source = Path(__file__).parent
        self.code_revision = hashlib.sha256(b"".join(path.read_bytes() for path in sorted(source.glob("*.py")))).hexdigest()[:12]+"-"+uuid.uuid4().hex[:8]
        self.observations = {"usage": True, "codex_account": False, "codex": codex, "jev": True, "metadata": True, "git": True, "worktrees": True, "jev_calls": True, "skills": True, "checks": True, "tool_events": True, "mcp": True, "web": True, "files": True, "errors": True, "logs": True, "sqlite": True, "model_api": True}
        self.default_settings = {"interval": 10, "idle_minutes": 5, "activity_retention_days": ActivityHistory.DEFAULT_DAYS, "max_files": 20, "track_all": False, "observations": dict(self.observations), "mcp_sources": {}, "mcp_categories": {}, "tool_descriptions": {}, "mcp_descriptions": {}, "mcp_tags": {}}
        self.mcp_sources, self.mcp_categories, self.tool_descriptions, self.mcp_descriptions, self.mcp_tags = {}, {}, {}, {}, {}
        self.mcp_document_cache = {}
        self.started_at = now()
        self.monitor = MonitorState(home/"monitoring/local-activity-monitor.jsonl", defer_device=True)
        self.worktrees = WorktreeCollector(home)
        self.diagnostics = DiagnosticCollector(home)
        self.error_history = ErrorHistory(home/"monitoring/error-history.json")
        self.activity_history = ActivityHistory(home/"monitoring/activity-history.json")
        if self.codex:
            self.codex.history_sink = self.retain_calls
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.refresh_lock = threading.RLock()
        self.update_condition = threading.Condition()
        self.update_version = 0
        self.event_clients = threading.BoundedSemaphore(8)
        self.cache = {}
        self.activity_cache = self.activity.snapshot(self.idle_minutes, self.interval)
        self.asset_signature, self.asset_revision = None, None
        self.stagger = False
        self.phase_times = {}
        self.phase_started = None
        self.thread = threading.Thread(target=self.poll, name="metadata-collectors", daemon=True)

    def preload(self):
        """Read bounded catalog metadata before the first session collection."""
        if not self.codex or not self.observations["codex"] or not self.observations["metadata"]:
            return
        try:
            projections = self.codex.snapshot_windows()
            base = {"label": "本機觀察統計", "health": "starting", "started_at": self.started_at,
                    "updated_at": None, "initial_sections": ["catalog", "runtime"], "settings": self.settings(),
                    "availability": {}, "mcp": {"servers": [], "events": [], "categories": CATEGORIES},
                    "jev": {"health": "waiting", "enabled": False, "summary": {}, "recent": [], "series": []}}
            with self.lock:
                self.cache = {window: base | {"codex": projection} for window, projection in projections.items()}
        except (OSError, ValueError, sqlite3.Error):
            self.monitor.event("preload_failed")

    def retain_calls(self, states):
        sql, web, mcp, errors = [], [], [], []
        for state in states:
            errors.extend(event|{'thread_id':state['thread_id']} for event in state.get('errors',[]))
            for identity, call in state['calls'].items():
                stamp, end = call.get('timestamp'), call.get('completed_at')
                duration = max(0, round((datetime.fromisoformat(end.replace('Z','+00:00'))-datetime.fromisoformat(stamp.replace('Z','+00:00'))).total_seconds()*1000)) if stamp and end else None
                context = {'thread_id':state['thread_id'], 'call_id':identity, 'timestamp':stamp, 'completed_at':end}
                sql.extend(context|event|{'index':index, 'duration_ms':None, 'container_duration_ms':duration, 'result':'failed' if call.get('error') else 'returned' if end else 'unknown'} for index,event in enumerate(call.get('sqlite',[])))
                for index,event in enumerate(call.get('mcp',[])):
                    value = event|context|{'index':index, 'duration_ms':None if event['nested'] else duration, 'container_duration_ms':duration if event['nested'] else None}
                    (web if event['server']=='web' else mcp).append(value)
                if call.get('error'):
                    errors.append(call['error']|context|{'timestamp':end or stamp, 'category':'tool', 'source':'tool_result', 'severity':'error', 'tool':call['tool']})
        self.activity_history.update(sql,web,mcp)
        self.error_history.update(errors)
        self.codex.thread_state.update(states)
        return self.activity_history.health=='ok' and self.error_history.health=='ok' and self.codex.thread_state.write_health=='ok'

    def refresh(self, stagger=False):
        try:
            with self.refresh_lock:
                self.stagger = stagger
                self.phase_times = {}
                self.phase_started = None
                self._refresh()
                self.phase("idle")
            self.notify_update()
        except Exception as error:
            self.monitor.collecting("error")
            self.monitor.failed(error)
            self.notify_update()
            raise

    def notify_update(self):
        with self.update_condition:
            self.update_version += 1
            self.update_condition.notify_all()

    def phase(self, name):
        if self.phase_started is not None:
            previous, wall, cpu = self.phase_started
            self.phase_times[previous] = {"elapsed_ms": round((time.perf_counter()-wall)*1000, 2), "cpu_ms": round((time.process_time()-cpu)*1000, 2)}
        if self.stagger and self.stop.wait(.05):
            self.monitor.collecting("stopped")
            return False
        self.phase_started = (name, time.perf_counter(), time.process_time())
        self.monitor.collecting(name)
        return True

    def web_revision(self):
        assets = load_frontend_assets().root
        library = load_ui_assets()
        paths = [assets/name for name in ("index.html", "app.js", "style.css", "locales.json")]+[library.path(name) for name in ("workbench-ui.js", "workbench-ui.css")]
        signature = [(str(path), path.stat().st_mtime_ns, path.stat().st_size) for path in paths]
        if signature != self.asset_signature:
            self.asset_revision = self.code_revision+"-"+hashlib.sha256(b"".join(path.read_bytes() for path in paths)).hexdigest()[:12]
            self.asset_signature = signature
        return self.asset_revision

    def _refresh(self):
        with self.refresh_lock:
            began, cpu = time.perf_counter(), time.process_time()
            if not self.phase("sessions"):
                return
            before_bytes = (self.codex.read_bytes if self.codex else 0)+self.diagnostics.read_bytes
            if self.codex and self.observations["codex"]:
                previous = self.cache.get("all", {}).get("codex", {})
                targets = (self.activity_history.snapshot("sql") if self.observations["sqlite"] else [])+(self.activity_history.snapshot("mcp") if self.observations["mcp"] else [])+(self.activity_history.snapshot("web") if self.observations["web"] else [])
                if self.observations["git"]:
                    targets.extend(previous.get("git", {}).get("events", []))
                if self.observations["checks"]:
                    targets.extend(previous.get("checks", []))
                if self.observations["tool_events"]:
                    targets.extend(event | {"thread_id": thread["thread_id"]} for thread in previous.get("threads", []) for event in thread.get("tool_events", []))
                self.codex.select_detail_targets(targets)
                self.codex.refresh(pause=(lambda: self.stop.wait(.01)) if self.stagger else None)
                if self.stop.is_set():
                    return
            if not self.phase("projections"):
                return
            codex_windows = self.codex.snapshot_windows(pause=lambda: self.stop.wait(.05) if self.stagger else False) if self.codex and self.observations["codex"] else {window: {"source": "codex", "health": "disabled", "threads": [], "tools": {}} for window in WINDOWS}
            if self.stop.is_set():
                return
            if not self.phase("worktrees"):
                return
            worktrees = self.worktrees.refresh(getattr(self.codex, "project_details", {}), codex_windows["all"].get("threads", []),
                                               getattr(self.codex, "thread_workdirs", {}), self.observations["codex"] and self.observations["worktrees"])
            for projection in codex_windows.values():
                projection["worktrees"] = worktrees
            if not self.phase("account"):
                return
            # This refresh worker may wait on the optional source, while readers
            # continue receiving the last snapshot without holding self.lock.
            account = self.account.snapshot(self.observations["codex_account"] and self.observations["usage"])
            for projection in codex_windows.values():
                projection["account"] = account
                if account["methods"]["account/rateLimits/read"]["health"] == "ok" and account["limits"]:
                    local = projection.get("usage") or {}
                    bucket = account["limits"].get(local.get("limit_id")) or account["limits"].get("codex") or next(iter(account["limits"].values()))
                    projection["usage"] = {"source": "codex_app_server", "updated_at": account["updated_at"], "limit_id": bucket["limit_id"], "plan_type": bucket.get("plan_type") or account.get("plan_type") or local.get("plan_type"), "credits": bucket.get("credits") if bucket.get("credits") is not None else account.get("credits"), "limits": [bucket[key] for key in ("primary", "secondary") if bucket.get(key) is not None]}
            codex_windows = {window: dict(projection) for window, projection in codex_windows.items()}
            codex = codex_windows["all"]
            enabled, database, since = monitor_config(self.home)
            if not self.phase("diagnostics"):
                return
            self.diagnostics.capture_logs = self.observations["logs"]
            self.diagnostics.capture_sql = self.observations["sqlite"]
            self.diagnostics.capture_api = self.observations["model_api"]
            if self.observations["codex"] and (self.observations["errors"] or self.observations["logs"] or self.observations["sqlite"] or self.observations["model_api"]):
                self.diagnostics.refresh()
            if self.observations["codex"]:
                if not self.phase("history"):
                    return
                sql = codex.get("sqlite", {}).get("_retained_events", codex.get("sqlite", {}).get("events", []))+list(self.diagnostics.sql_events) if self.observations["sqlite"] else []
                web = [event for event in codex.get("mcp_events", []) if event["server"]=="web"] if self.observations["web"] else []
                mcp = [event for event in codex.get("mcp_events", []) if event["server"]!="web"] if self.observations["mcp"] else []
                self.activity_history.update(sql, web, mcp)
                history = (self.activity_history.snapshot("web") if self.observations["web"] else [])+(self.activity_history.snapshot("mcp") if self.observations["mcp"] else [])
                for window, projection in codex_windows.items():
                    retained = {}
                    for event in [item for item in history if contains(item, cutoff(window))]+projection.get("mcp_events", []):
                        identity = mcp_identity(event)
                        retained[identity] = merge_event(retained.get(identity, {}), event)
                    projection["mcp_events"] = list(retained.values())
            if self.observations["codex"] and self.observations["sqlite"]:
                history_sql = self.activity_history.snapshot("sql")
                for window, projection in codex_windows.items():
                    sql = projection.setdefault("sqlite", {"events": [], "total": 0, "operations": {}})
                    retained = [event for event in history_sql if contains(event, cutoff(window))]+sql.pop("_retained_events", sql["events"])+[event for event in self.diagnostics.sql_events if contains(event, cutoff(window))]
                    merged = {}
                    for event in retained:
                        identity = sql_identity(event)
                        merged[identity] = merge_event(merged.get(identity, {}), event)
                    retained = list(merged.values())
                    events = sorted(retained, key=lambda event:event.get("timestamp") or "", reverse=True)[:CodexCollector.SQL_EVENT_LIMIT]
                    identity_fields = ("source", "thread_id", "call_id", "index", "file", "record_id", "record_offset", "record_hash", "timestamp", "statement")
                    events = [event | {"id": hashlib.sha256(json.dumps([event.get(key) for key in identity_fields]).encode()).hexdigest()} for event in events]
                    sql.update(events=events, total=len(retained), operations=dict(Counter(event["operation"] for event in retained)))
                    boundary = cutoff(window)
                    sql['aggregate'] = self.activity_history.store.aggregate('sql', since=boundary.isoformat() if boundary else None) | {'scope':'retained_database', 'window':window}
            for projection in codex_windows.values():
                projection.get("sqlite", {}).pop("_retained_events", None)
            diagnostics = self.diagnostics.snapshot() if self.observations["codex"] and self.observations["errors"] else {"events": [], "health": {"desktop": "disabled", "core": "disabled"}}
            diagnostic_events = diagnostics.pop("events")
            errors = codex.get("error_events", [])+diagnostic_events
            descriptions = {}
            if not self.phase("mcp"):
                return
            sources = discover_sources(self.home, descriptions, self.mcp_document_cache)
            if len(self.mcp_document_cache) > 256:
                self.mcp_document_cache.clear()
            mcp = summarize(sources, codex.get("mcp_events", []), self.mcp_sources, self.mcp_categories, errors)
            mcp["configuration"] = {"location": str(self.home/"config.toml"), "configured_sources": len(sources)}
            for source in mcp["servers"]:
                source.update(descriptions.get(source["server"], {}))
                source["default_description"] = source.get("description", "")
                if source["server"] in self.mcp_descriptions:
                    source.update(description=self.mcp_descriptions[source["server"]], description_source="custom")
                source["default_category"] = category(source["server"], " ".join(source["tools"])+" "+source.get("default_description", ""))
                source["category"] = self.mcp_categories.get(source["server"]) or source["default_category"]
                if source["server"] in self.mcp_tags:
                    source["tags"] = list(self.mcp_tags[source["server"]])
            availability = {"jev": "jev" in sources or database is not None or any(item["server"] == "jev" for item in mcp["servers"])}
            settings = self.settings()
            revision = self.web_revision()
            scoped_mcp = {}
            for window, projection in codex_windows.items():
                report = summarize(sources, projection.pop("mcp_events", []), self.mcp_sources, self.mcp_categories)
                complete = {source["server"]: source for source in mcp["servers"]}
                report["servers"] = [complete[source["server"]] | source | {"connection": complete[source["server"]]["connection"], "category": complete[source["server"]]["category"]} for source in report["servers"]]
                # Configured and observed sources remain available even with no activity in this window.
                present = {source["server"] for source in report["servers"]}
                report["servers"] += [source | {"calls": 0, "recognized": 0, "returned": 0, "known_status": 0, "errors": 0, "average_ms": None, "p99_ms": None, "p95_ms": None, "last_at": None, "latest_metrics": {}} for source in mcp["servers"] if source["server"] not in present]
                report["configuration"] = mcp["configuration"]
                report["recording_status"] = mcp["recording_status"]
                if self.observations['codex'] and self.observations['mcp']:
                    boundary = cutoff(window)
                    report['history_aggregate'] = self.activity_history.store.aggregate('mcp', since=boundary.isoformat() if boundary else None, excluded_sources=[key for key,enabled in self.mcp_sources.items() if enabled is False]) | {'scope':'retained_database', 'window':window}
                scoped_mcp[window] = report
            connection = connection_status(codex.get("threads", []), codex.get("error_events", [])+diagnostic_events, self.observations["codex"])
            if not self.phase("jev"):
                return
            for projection in codex_windows.values():
                projection["connection"] = connection
            cache = {window: {"version": 1, "revision": revision, "label": "本機觀察統計", "started_at": self.started_at, "updated_at": now(), "settings": settings, "default_settings": self.default_settings, "availability": availability, "mcp": scoped_mcp[window], "jev": self.jev.snapshot(window) if self.observations["jev"] and self.observations["mcp"] and self.mcp_sources.get("jev", True) else {"source": "jev", "health": "paused", "scope": self.jev.SCOPE, "enabled": enabled, "enabled_at": since, "summary": {}, "recent": [], "series": []}, "codex": projection} for window, projection in codex_windows.items()}
            for window, snapshot in cache.items():
                api_enabled = self.observations["codex"] and self.observations["model_api"]
                snapshot["model_api"] = api_summary([event for event in self.diagnostics.api_events if contains(event, cutoff(window))] if api_enabled else []) | {"enabled": api_enabled, "sources": self.diagnostics.log_sources(api_enabled)}
                snapshot["frontend_revision"] = revision.rsplit("-", 1)[-1]
                snapshot["backend_revision"] = self.code_revision
                statuses = dict(mcp["recording_status"])
                if availability["jev"]:
                    state = copy.deepcopy(statuses.get("jev", {"enabled": None, "health": None, "observed_at": None, "flags": {}}))
                    state["collector_health"] = snapshot["jev"]["health"]
                    if database is not None:
                        config_flag = {"enabled": enabled, "observed_at": since, "health": None, "source": "monitor_config"}
                        state.setdefault("flags", {})["config.recording_enabled"] = config_flag
                        if state.get("enabled") is None:
                            state.update(config_flag, key="config.recording_enabled")
                    statuses["jev"] = state
                snapshot["mcp"]["recording_status"] = statuses
                snapshot["mcp"]["telemetry"] = {"jev": {key: snapshot["jev"][key] for key in ("enabled", "enabled_at", "health", "scope", "summary", "recent", "series")} | {"window": snapshot["jev"].get("window"), "reader": {"locations": [str(database)] if database else [], "record_limit": self.jev.RECENT_LIMIT, "series_limit": self.jev.SERIES_LIMIT}}} if availability["jev"] else {}
                jev_errors = []
                if self.observations["errors"]:
                    for event in snapshot["jev"]["recent"]:
                        for index, attempt in enumerate(event.get("attempts", [])):
                            if attempt.get("status") in FAILURES:
                                jev_errors.append({"timestamp": event["timestamp"], "category": "mcp", "source": "jev_telemetry", "severity": "error", "server": "jev", "tool": event["operation"], "code": attempt["status"], "http_status": attempt.get("http_status"), "attempt": index+1})
                window_errors = snapshot["codex"].pop("error_events", [])+[event for event in diagnostic_events if contains(event, cutoff(window))]
                snapshot["errors"] = error_summary(window_errors+jev_errors) | {"diagnostics": diagnostics, "enabled": self.observations["errors"]}
                snapshot["_retained_error_events"] = window_errors+jev_errors
            history = [event for snapshot in cache.values() for event in snapshot["errors"]["events"]]
            history += [{"timestamp": event["timestamp"], "category": "monitor", "source": "monitor", "severity": "error", "code": event.get("error_type") or "http_error", "http_status": event.get("http_status"), "reason": event["kind"]} for event in self.monitor.log_snapshot()["entries"] if event["kind"] in ("refresh_failed", "http_response_error")]
            self.error_history.update(history)
            database_bytes = None
            if not self.phase("hardware"):
                return
            self.monitor.load_device()
            if not self.phase("complete"):
                return
            if database is not None:
                try:
                    database_bytes = database.stat().st_size
                except OSError:
                    pass
            self.monitor.refreshed({"phase_metrics": dict(self.phase_times), "max_phase_cpu_ms": max((phase["cpu_ms"] for phase in self.phase_times.values()), default=0), "refresh_ms": round((time.perf_counter()-began)*1000, 2), "cpu_ms": round((time.process_time()-cpu)*1000, 2), "read_bytes": (self.codex.read_bytes if self.codex else 0)+self.diagnostics.read_bytes-before_bytes, "retained_calls": sum(len(state["calls"]) for state in self.codex.files.values()) if self.codex else 0, "buffer_bytes": sum(len(state["buffer"]) for state in self.codex.files.values()) if self.codex else 0, "trimmed_calls": self.codex.trimmed_calls if self.codex else 0, "call_limit": CodexCollector.CALL_LIMIT, "file_limit": CodexCollector.FILE_LIMIT, "buffer_limit": CodexCollector.BUFFER_LIMIT, "jev_database_bytes": database_bytes, "activity_cache_bytes": len(self.activity_history.saved) if self.activity_history.saved is not None else None, "sql_records": len(self.activity_history.sql), "web_records": len(self.activity_history.web), "mcp_records": len(self.activity_history.mcp)})
            with self.lock:
                self.cache = cache

    def set_jev_recording(self, enabled):
        with self.refresh_lock:
            configure(self.jev.home, enabled)
            self.monitor.event("recording_enabled" if enabled else "recording_disabled")
            self.refresh()
            current = self.snapshot("24h")["jev"]
            return {"enabled": current["enabled"], "enabled_at": current["enabled_at"]}

    def settings(self):
        return {"interval": self.interval, "idle_minutes": self.idle_minutes, "activity_retention_days": self.activity_history.retention_days, "max_files": self.max_files, "track_all": self.codex.track_all if self.codex else False, "observations": dict(self.observations), "mcp_sources": dict(self.mcp_sources), "mcp_categories": dict(self.mcp_categories), "tool_descriptions": dict(self.tool_descriptions), "mcp_descriptions": dict(self.mcp_descriptions), "mcp_tags": {key: list(tags) for key, tags in self.mcp_tags.items()}}

    def set_settings(self, value):
        allowed = {"interval", "idle_minutes", "activity_retention_days", "max_files", "track_all", "observations", "mcp_sources", "mcp_categories", "tool_descriptions", "mcp_descriptions", "mcp_tags", "replace_customizations", "recording"}
        if not isinstance(value, dict) or not value or set(value)-allowed:
            raise ValueError()
        observations = value.get("observations", {})
        if "interval" in value and (type(value["interval"]) is not int or not 1 <= value["interval"] <= 3600) or "track_all" in value and type(value["track_all"]) is not bool or not isinstance(observations, dict) or set(observations)-set(self.observations) or any(type(item) is not bool for item in observations.values()):
            raise ValueError()
        if "max_files" in value and (type(value["max_files"]) is not int or not 1 <= value["max_files"] <= 5000):
            raise ValueError()
        if "idle_minutes" in value and (type(value["idle_minutes"]) is not int or not 0 <= value["idle_minutes"] <= 1440):
            raise ValueError()
        if "activity_retention_days" in value and (type(value["activity_retention_days"]) is not int or not 0 <= value["activity_retention_days"] <= 3650):
            raise ValueError()
        if any(key in value and type(value[key]) is not bool for key in ("replace_customizations", "recording")):
            raise ValueError()
        for key in ("mcp_sources", "mcp_categories"):
            items = value.get(key, {})
            if not isinstance(items, dict) or len(items) > 64 or any(not isinstance(name, str) or not SOURCE.fullmatch(name) for name in items):
                raise ValueError()
            if key == "mcp_sources" and any(type(item) is not bool for item in items.values()) or key == "mcp_categories" and any(not isinstance(item, str) or item not in CATEGORIES for item in items.values()):
                raise ValueError()
        descriptions = value.get("tool_descriptions", {})
        if not isinstance(descriptions, dict) or len(descriptions) > 64 or any(not tool_name(key) or not isinstance(text, str) or len(text) > 400 for key, text in descriptions.items()):
            raise ValueError()
        purposes = value.get("mcp_descriptions", {})
        if not isinstance(purposes, dict) or len(purposes) > 64 or any(not isinstance(key, str) or not SOURCE.fullmatch(key) or not isinstance(text, str) or len(text) > 400 for key, text in purposes.items()):
            raise ValueError()
        tags = value.get("mcp_tags", {})
        if not isinstance(tags, dict) or len(tags) > 64 or any(not isinstance(key, str) or not SOURCE.fullmatch(key) or not isinstance(items, list) or len(items) > 4 or any(not isinstance(text, str) or not text.strip() or len(text.strip()) > 40 or any(ord(char) < 32 for char in text) for text in items) for key, items in tags.items()):
            raise ValueError()
        with self.refresh_lock:
            replace = value.get("replace_customizations", False)
            next_descriptions = {key: text for key, text in (descriptions if replace else self.tool_descriptions | descriptions).items() if text.strip()}
            next_purposes = {key: text for key, text in (purposes if replace else self.mcp_descriptions | purposes).items() if text.strip()}
            next_tags = {key: list(dict.fromkeys(text.strip() for text in items)) for key, items in (tags if replace else self.mcp_tags | tags).items() if items}
            if len(next_purposes) > 64 or len(next_tags) > 64:
                raise ValueError()
            if not replace and any(len(set(existing)|set(value.get(key, {}))) > 64 for key, existing in (("mcp_sources", self.mcp_sources), ("mcp_categories", self.mcp_categories))) or len(next_descriptions) > 64:
                raise ValueError()
            if "recording" in value:
                changed = configure(self.jev.home, value["recording"]) if value["recording"] or monitor_config(self.jev.home)[1] is not None else "unchanged"
                if changed != "unchanged":
                    self.monitor.event("recording_enabled" if value["recording"] else "recording_disabled")
            interval = value.get("interval", self.interval)
            track_all = value.get("track_all", self.codex.track_all if self.codex else False)
            if observations.get("codex") and not self.codex:
                self.codex = CodexCollector(self.home/"sessions", self.max_files)
                self.codex.history_sink = self.retain_calls
            self.interval = interval
            self.idle_minutes = value.get("idle_minutes", self.idle_minutes)
            if "activity_retention_days" in value:
                self.activity_history.retention_days = value["activity_retention_days"]
                self.activity_history.update([], [], [])
            self.activity.wake()
            self.cache_activity()
            self.max_files = value.get("max_files", self.max_files)
            restart_diagnostics = any(observations.get(key) and not self.observations[key] for key in ("codex", "errors", "logs", "sqlite", "model_api"))
            self.observations.update(observations)
            if restart_diagnostics:
                self.diagnostics.files.clear()
                self.diagnostics.next_scan = 0
                self.diagnostics.sql_cursor = self.diagnostics.sql_history = None
                self.diagnostics.events.clear()
                self.diagnostics.logs.clear()
                self.diagnostics.sql_events.clear()
                self.diagnostics.api_events.clear()
            self.monitor.log_enabled = self.observations["logs"]
            if observations.get("logs") is False:
                self.diagnostics.logs.clear()
            if observations.get("sqlite") is False:
                self.diagnostics.sql_events.clear()
            if observations.get("model_api") is False:
                self.diagnostics.api_events.clear()
            source_changes = value.get("mcp_sources", {})
            next_sources = dict(source_changes) if replace else self.mcp_sources | source_changes
            reenable = any(next_sources.get(name, True) and enabled is False for name, enabled in self.mcp_sources.items())
            self.mcp_sources = next_sources
            self.mcp_categories = dict(value.get("mcp_categories", {})) if replace else self.mcp_categories | value.get("mcp_categories", {})
            self.tool_descriptions = next_descriptions
            self.mcp_descriptions = next_purposes
            self.mcp_tags = next_tags
            if self.codex:
                if reenable or any(enabled and not self.codex.features[key] for key, enabled in observations.items() if key in self.codex.features):
                    self.codex.files.clear()
                    self.codex.next_scan = 0
                self.codex.features.update({key: enabled for key, enabled in self.observations.items() if key in self.codex.features})
                self.codex.mcp_sources = dict(self.mcp_sources)
                if self.codex.max_files != self.max_files:
                    self.codex.max_files = self.max_files
                    self.codex.next_scan = 0
                if self.codex.track_all != track_all:
                    self.codex.track_all = track_all
                    self.codex.next_scan = 0
            self.monitor.event("settings_applied")
            self.refresh()
            return self.settings()

    def sql_detail(self, identity, mask=True, full=False):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"] or not self.observations["sqlite"]:
                return None
            event = next((event for event in self.cache.get("all", {}).get("codex", {}).get("sqlite", {}).get("events", []) if event.get("id") == identity), None)
            if event is None:
                return None
            return self.diagnostics.sql_detail(event, mask, full) if event.get("recognition") == "diagnostic_log" else self.codex.sql_detail(event, mask, full)

    def error_detail(self, identity, full=False):
        with self.refresh_lock:
            if not self.observations["codex"] or not (self.observations["errors"] or self.observations["logs"]):
                return None
            observed = list(self.diagnostics.events)+list(self.diagnostics.logs)+self.error_history.snapshot()+self.cache.get("all", {}).get("_retained_error_events", [])
            event = next((item for item in observed if item.get("content_id") == identity), None)
            if not event:
                return None
            return self.codex.error_detail(event, full) if self.codex and event.get("source") in ("session", "tool_result") else self.diagnostics.error_detail(event, full)

    def jev_detail(self, thread_id, call_id, index, full=False):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"] or not self.observations["jev_calls"]:
                return None
            return self.codex.jev_detail(thread_id, call_id, index, full)

    def git_detail(self, thread_id, call_id, operation, full=False):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"] or not self.observations["git"]:
                return None
            return self.codex.git_detail(thread_id, call_id, operation, full)

    def tool_detail(self, thread_id, call_id, full=False):
        with self.refresh_lock:
            if not self.codex or not self.observations['codex'] or not self.observations['tool_events']:
                return None
            return self.codex.tool_detail(thread_id, call_id, full)

    def check_detail(self, thread_id, call_id, operation, full=False):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"] or not self.observations["checks"]:
                return None
            return self.codex.check_detail(thread_id, call_id, operation, full)

    def mcp_detail(self, thread_id, call_id, index, full=False):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"]:
                return None
            event = next((item for item in self.cache.get("all", {}).get("mcp", {}).get("events", []) if item.get("thread_id") == thread_id and item.get("call_id") == call_id and item.get("index") == index), None)
            if not event or not self.observations["web" if event["server"] == "web" else "mcp"] or self.mcp_sources.get(event["server"]) is False:
                return None
            return self.codex.mcp_detail(thread_id, call_id, index, full, event)

    def mcp_documents(self, server, document=None):
        with self.refresh_lock:
            configurations = {}
            discover_sources(self.home, configurations=configurations)
            value = configurations.get(server)
            if value is None:
                return None
            if document:
                return read_document(value, document)
            return {"server": server, "files": list(source_documents(value).values())}

    def save_mcp_document(self, server, document, text, expected):
        with self.refresh_lock:
            configurations = {}
            discover_sources(self.home, configurations=configurations)
            value = configurations.get(server)
            if value is None:
                raise ValueError()
            result = write_document(value, document, text, expected)
            self.mcp_document_cache.clear()
            self.activity.wake()
            self.cache_activity()
            self.refresh()
            return result

    def skill_detail(self, skill, document=None, thread_id=None, call_id=None):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"] or not self.observations["skills"]:
                return None
            return self.codex.skill_detail(skill, document, thread_id, call_id)

    def project_detail(self, project_id):
        with self.refresh_lock, self.lock:
            codex = self.cache.get("all", {}).get("codex", {})
            project = next((item for item in codex.get("projects", []) if item["id"] == project_id), None)
            if project is None:
                return None
            detail = getattr(self.codex, "project_details", {}).get(project_id, {}) if self.codex else {}
            threads = [{key: row.get(key) for key in ("thread_id", "thread_name", "environment", "status", "archived", "model", "updated_at")} for row in codex.get("threads", []) if row.get("project_id") == project_id]
            return copy.deepcopy(project | {"project_id": project_id, "folders": detail.get("folders", []), "source": detail.get("source"), "threads": threads})

    def worktree_detail(self, identity):
        with self.refresh_lock:
            if not self.observations["codex"] or not self.observations["worktrees"]:
                return None
            return copy.deepcopy(self.worktrees.detail(identity))

    def file_detail(self, thread_id, call_id, path):
        with self.lock:
            if not self.observations["codex"] or not self.observations["files"]:
                return None
            codex = self.cache.get("all", {}).get("codex", {})
            event = next((item for item in codex.get("file_activity", {}).get("events", []) if item.get("thread_id") == thread_id and item.get("call_id") == call_id and item.get("path") == path), None)
        return observed_file_metadata(event) if event else None

    def file_summary(self, window):
        with self.lock:
            if not self.observations["codex"] or not self.observations["files"]:
                return None
            events = self.cache.get(window, {}).get('codex', {}).get('file_activity', {}).get('events', [])
            selected = {}
            for event in events:
                selected.setdefault((event.get('workdir'), event['path']), dict(event))
            total = len(selected)
            selected = list(selected.values())[:100]
        files = [event | observed_file_metadata(event) for event in selected]
        return {'files': files, 'checked_at': now(), 'limit': 100, 'total': total}

    def instruction_detail(self, scope, project_id=None):
        if scope == "global" and project_id is None:
            return instructions([self.home]) | {"scope": "global"}
        if scope == "project" and project_id:
            project = self.project_detail(project_id)
            if project is not None:
                return instructions(project["folders"]) | {"scope": "project", "project_id": project_id}
        return None

    def logs(self, window="all"):
        """List selected metadata; known diagnostic bodies are read separately on demand."""
        with self.refresh_lock:
            monitor = self.monitor.log_snapshot()
            entries = list(self.diagnostics.logs) if self.observations["logs"] and self.observations["codex"] else []
            entries += monitor.pop("entries")
            with self.lock:
                cached = self.cache.get("all", {})
                codex, jev = cached.get("codex", {}), cached.get("jev", {})
                errors = cached.get("_retained_error_events", cached.get("errors", {}).get("events", []))+self.error_history.snapshot()
                if self.observations["logs"]:
                    entries += errors
            boundary = cutoff(window)
            entries = [event for event in {error_identity(event): event for event in entries}.values() if contains(event, boundary)]
            entries.sort(key=lambda event: event.get("timestamp") or "", reverse=True)
            return {"enabled": self.observations["logs"], "entries": copy.deepcopy(entries[:2000]), "total": len(entries), "limit": 2000, "trimmed": self.diagnostics.log_trimmed, "sources": self.diagnostics.log_sources(self.observations["codex"] and self.observations["logs"])+[monitor, {"source": "session", "health": codex.get("health", "waiting"), "files": [], "file_count": codex.get("files"), "checked_at": cached.get("updated_at"), "read_bytes": codex.get("bytes_read"), "unsupported_lines": codex.get("malformed_lines"), "backfill_pending": codex.get("error_backfill_pending")}, {"source": "jev_telemetry", "health": jev.get("health", "waiting"), "files": [], "checked_at": cached.get("updated_at")}]}

    def activity_status(self):
        # Readers use the last complete activity check instead of waiting for scans.
        with self.lock:
            return dict(self.activity_cache) | {"code_revision": self.code_revision}

    def cache_activity(self):
        with self.lock:
            value = self.activity.snapshot(self.idle_minutes, self.interval)
            changed = self.activity_cache.get('paused') != value.get('paused')
            self.activity_cache = value
        if changed:
            self.notify_update()

    def resume_refresh(self):
        with self.refresh_lock:
            self.activity.wake()
            self.cache_activity()
            self.refresh()
            return self.activity_status()

    def poll_once(self):
        with self.refresh_lock:
            self.activity.check(self.codex if self.observations["codex"] else None, self.idle_minutes, self.observations["metadata"])
            self.cache_activity()
            if self.activity.paused:
                return False
            self.refresh(stagger=True)
            return True

    def poll(self):
        while not self.stop.is_set():
            try:
                self.poll_once()
            except Exception:
                # Collector errors cannot expose record content or exception paths
                pass
            self.stop.wait(self.activity_status()["check_interval"])

    def snapshot(self, window):
        with self.lock:
            cached = self.cache.get(window)
            result = copy.deepcopy(cached) if cached is not None else {"label": "本機觀察統計", "health": "starting", "started_at": self.started_at, "updated_at": None, "settings": self.settings(), "availability": {}, "mcp": {"servers": [], "events": [], "categories": CATEGORIES}, "codex": {"health": "waiting", "threads": [], "tools": {}}, "jev": {"health": "disabled", "enabled": False, "summary": {}, "recent": [], "series": []}}
        result["default_settings"] = copy.deepcopy(self.default_settings)
        result["activity"] = self.activity_status()
        result["monitor"] = self.monitor.snapshot()
        monitor_boundary = cutoff(window)
        result["monitor"]["history"] = [sample for sample in result["monitor"]["history"] if contains({"timestamp": sample.get("time")}, monitor_boundary)]
        result["monitor"]["events"] = [event for event in result["monitor"]["events"] if contains(event, monitor_boundary)]
        result["monitor"].update(window=window, sample_scope="retained_samples_in_selected_window", unknown_timestamp="all_only")
        logs = self.monitor.log_snapshot(False)
        result["logs"] = {"enabled": self.observations["logs"], "sources": self.diagnostics.log_sources(self.observations["codex"] and self.observations["logs"])+[logs], "retained": len(self.diagnostics.logs)+logs["retained"], "trimmed": self.diagnostics.log_trimmed}
        errors = result.setdefault("errors", {"events": [], "diagnostics": {}, "enabled": self.observations["errors"]})
        program_errors = [{"timestamp": event["timestamp"], "category": "monitor", "source": "monitor", "severity": "error", "code": event.get("error_type") or "http_error", "http_status": event.get("http_status"), "reason": event["kind"]} for event in result["monitor"]["events"] if event["kind"] in ("refresh_failed", "http_response_error", "frontend_error")]
        history = self.error_history.snapshot() if self.observations["errors"] else []
        retained_errors = result.pop("_retained_error_events", errors["events"])
        merged = {error_identity(event):event for event in history+retained_errors+program_errors}
        boundary = cutoff(window)
        errors.update(error_summary([event for event in merged.values() if contains(event, boundary)]))
        errors["history"] = {"health": self.error_history.health, "error_type": self.error_history.error_type, "retained": len(history), "limit": self.error_history.LIMIT, "hours": 24}
        result["sources"] = self.sources(result)
        for source in result["sources"].values():
            source["display_window"] = window
        return result

    def sources(self, snapshot=None):
        """Describe loaded readers without another scan or reading source bodies."""
        snapshot = snapshot or {}
        codex, mcp = snapshot.get("codex", {}), snapshot.get("mcp", {})
        read = codex.get("read_state", {})
        active = self.observations["codex"]
        registry = {}
        def add(key, name, features, scope, locations=(), health=None, fields=(), limits=None, readers=()):
            registry[key] = {"name": name, "features": features, "scope": scope, "locations": list(locations),
                             "mode": "read_only", "fields": list(fields), "limits": limits or {}, "readers": list(readers)}
            if health is not None:
                registry[key]["health"] = health
        add("session", "Codex session", ["codex", "tools", "git", "workflow", "skills", "checks", "files", "sqlite", "mcp", "web", "errors", "logs", "usage", "dots"],
            "recent_tail_and_incremental_metadata", [str(self.home/"sessions"), *read.get("locations", [])],
            codex.get("health", "waiting") if active else "disabled", (),
            {key: value for key, value in read.items() if isinstance(value, (int, bool))} | {"read_bytes": codex.get("bytes_read"), "unsupported_lines": codex.get("malformed_lines")})
        registry["session"]["observations"] = read.get("enabled_features", [])
        readers = codex.get("metadata_sources", [])
        loaded = [reader for reader in readers if reader["health"] == "ok"]
        catalog_health = "disabled" if not active or not self.observations["metadata"] else "ok" if loaded and len(loaded) == len(readers) else "partly_unavailable" if loaded else "waiting" if not readers else "unavailable"
        add("catalog", "Codex / ChatGPT catalog", ["codex", "usage", "dots", "workflow", "skills", "plugins"],
            "titles_classification_and_local_thread_metadata", [reader["location"] for reader in readers], catalog_health,
            sorted({field for reader in loaded for field in reader.get("fields", [])}), readers=readers)
        worktrees = codex.get("worktrees", {})
        add("worktrees", "Git worktrees", ["projects", "worktrees"], "loaded_project_roots_and_codex_managed_worktrees",
            [str(self.worktrees.managed_root)], worktrees.get("health", "disabled"),
            ["name", "branch", "commit", "detached", "locked", "prunable", "managed", "project_ids", "thread_ids", "checked_at"],
            {key: value for key, value in worktrees.items() if isinstance(value, (int, bool))})
        registry["worktrees"]["checked_at"] = worktrees.get("checked_at")
        account = codex.get("account", {})
        add("account_api", "Codex account API", ["usage"], "official_account_read_only", (), account.get("health", "disabled"),
            ["plan_type", "limits", "credits", "account_usage"], {"cache_seconds": 60, "timeout_seconds": 10, "output_bytes": 1048576},
            [{"name": method, "health": state["health"]} for method, state in account.get("methods", {}).items()])
        checkpoint = read.get("checkpoint", {})
        add("thread_state", "Thread lifecycle / Skills", ["codex", "workflow", "skills"], "confirmed_lifecycle_and_skill_checkpoints",
            [checkpoint["location"]] if checkpoint.get("location") else [], "disabled" if not active else checkpoint.get("write_health") or checkpoint.get("load_health"),
            ["thread_id", "status", "task_time", "task_start", "offset", "skill", "call_id"],
            {key: value for key, value in checkpoint.items() if type(value) is int},
            [{"name": "Checkpoint " + phase, "health": checkpoint[key]} for phase, key in (("load", "load_health"), ("save", "write_health")) if checkpoint.get(key)])
        registry["thread_state"]["mode"] = "bounded_metadata_cache"
        add('activity_history', 'Activity history', ['sqlite', 'web', 'mcp'], 'retained_activity_metadata',
            [str(self.activity_history.path)], self.activity_history.health if active else 'disabled',
            ['timestamp', 'thread_id', 'call_id', 'operation', 'status', 'duration_ms', 'reference_url'],
            {'sql_limit': self.activity_history.SQL_LIMIT, 'web_limit': self.activity_history.WEB_LIMIT, 'mcp_limit': self.activity_history.MCP_LIMIT,
             'byte_limit': self.activity_history.BYTE_LIMIT, 'retention_days': self.activity_history.retention_days,
             'sql_records': len(self.activity_history.sql), 'web_records': len(self.activity_history.web), 'mcp_records': len(self.activity_history.mcp)})
        registry['activity_history']['mode'] = 'bounded_metadata_cache'
        diagnostic_readers = self.diagnostics.log_sources(active and any(self.observations[key] for key in ("errors", "logs", "sqlite", "model_api")))
        add("diagnostics", "Codex diagnostics", ["errors", "logs", "sqlite", "model-api"], "recent_metadata_and_24h_error_backfill",
            [str(path) for path in self.diagnostics.files]+([str(self.diagnostics.sql_path)] if self.diagnostics.sql_path else []),
            "disabled" if not active or not any(self.observations[key] for key in ("errors", "logs", "sqlite")) else None,
            ["timestamp", "severity", "module", "code", "thread_id", "call_id", "request_id", "trace_id", "sql"],
            {"event_limit": self.diagnostics.EVENT_LIMIT, "file_limit": self.diagnostics.FILE_LIMIT}, diagnostic_readers)
        configuration = mcp.get("configuration", {})
        if configuration:
            add("mcp_config", "MCP configuration", ["mcp", "tools"], "configured_sources_and_purposes", [configuration["location"]],
                fields=["server", "enabled", "description", "category", "tags"], limits={"configured_sources": configuration.get("configured_sources"), "loaded_sources": len(mcp.get("servers", [])), "event_limit": MCP_EVENT_LIMIT, "tool_limit": MCP_TOOL_LIMIT})
        for server, telemetry in mcp.get("telemetry", {}).items():
            reader = telemetry.get("reader", {})
            add("telemetry:"+server, server+" telemetry", ["mcp", "errors", "logs"], telemetry.get("scope") or "reported_telemetry",
                reader.get("locations", []), telemetry.get("health"),
                sorted(telemetry.get("summary", {})), {"loaded_records": len(telemetry.get("recent", [])), "series_points": len(telemetry.get("series", [])), **{key: value for key, value in reader.items() if type(value) is int}})
            registry["telemetry:"+server]["window"] = telemetry.get("window")
            registry["telemetry:"+server]["checked_at"] = reader.get('checked_at') or telemetry.get('checked_at')
            registry["telemetry:"+server]["error_type"] = reader.get('error_type') or telemetry.get('error_type')
        monitor = snapshot.get("monitor", {})
        add("monitor", "Local Activity Monitor", ["monitor", "errors", "logs"], "runtime_samples_and_bounded_event_metadata",
            [str(self.monitor.journal), str(self.error_history.path)], monitor.get("health"),
            sorted(self.monitor.runtime), {"history_limit": monitor.get("history_limit"), "event_limit": monitor.get("event_limit"), "retained_samples": len(monitor.get("history", [])), "retained_events": len(monitor.get("events", [])), "byte_limit": self.monitor.JOURNAL_LIMIT*2, "error_history_limit": self.error_history.LIMIT, "error_history_byte_limit": self.error_history.BYTE_LIMIT})
        registry["monitor"]["mode"] = "bounded_metadata_cache"
        for key in ('session','monitor'):
            registry[key]['checked_at'] = snapshot.get('updated_at')
        return registry


def handler(dashboard, port):
    hosts = {f"127.0.0.1:{port}", f"localhost:{port}"}
    library = load_ui_assets()
    frontend = load_frontend_assets()
    assets = {"/": ("index.html", "text/html; charset=utf-8"), "/app.js": ("app.js", "text/javascript; charset=utf-8"), "/style.css": ("style.css", "text/css; charset=utf-8"), "/locales.json": ("locales.json", "application/json; charset=utf-8"), "/workbench-ui.js": ("workbench-ui.js", "text/javascript; charset=utf-8"), "/workbench-ui.css": ("workbench-ui.css", "text/css; charset=utf-8")}
    assets["/favicon.svg"] = ("favicon.svg", "image/svg+xml")
    assets["/favicon.ico"] = ("favicon.ico", "image/vnd.microsoft.icon")

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def setup(self):
            super().setup()
            self.connection.settimeout(15)

        def handle(self):
            try:
                super().handle()
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                self.close_connection = True

        def log_message(self, *args):
            pass

        def reply(self, code, content, content_type="text/plain; charset=utf-8", script_hash=None, style_hash=None):
            plain_bytes = len(content)
            compressed = len(content) >= 1024 and "gzip" in {item.strip() for item in self.headers.get("Accept-Encoding", "").split(",")}
            if compressed:
                content = gzip.compress(content, compresslevel=1)
            snapshot = getattr(self, "path", "").split("?", 1)[0] == "/api/snapshot" and code == 200
            dashboard.monitor.requested(code, plain_bytes if snapshot else None, len(content) if snapshot else None)
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(content)))
            if code >= 400 and (getattr(self, "command", None) != "GET" or self.headers.get("Content-Length") is not None or self.headers.get("Transfer-Encoding") is not None):
                self.close_connection = True
                self.send_header("Connection", "close")
            self.send_header("Vary", "Accept-Encoding")
            if compressed:
                self.send_header("Content-Encoding", "gzip")
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Referrer-Policy", "no-referrer")
            script_policy = " 'sha256-"+script_hash+"'" if script_hash else ""
            style_policy = " 'sha256-"+style_hash+"'" if style_hash else ""
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'"+script_policy+"; style-src 'self'"+style_policy+"; connect-src 'self'; img-src 'self'; frame-ancestors 'none'; base-uri 'none'")
            self.end_headers()
            self.wfile.write(content)

        def local_request(self):
            host, origin = self.headers.get("Host"), self.headers.get("Origin")
            if host not in hosts or origin is not None and origin != "http://"+host:
                self.reply(403, b"Local access only")
                return False
            if self.headers.get("Sec-Fetch-Site") == "cross-site":
                self.reply(403, b"Local access only")
                return False
            return True

        def content_request(self, url):
            query = parse_qs(url.query, keep_blank_values=True)
            kind = url.path.rsplit("/", 1)[-1]
            base = {"sql": {"id"}, "error": {"id"}, "tool": {"thread_id", "call_id"}, "mcp": {"thread_id", "call_id", "index"}, "jev": {"thread", "call", "index"}, "git": {"thread_id", "call_id", "operation"}, "check": {"thread_id", "call_id", "operation"}}[kind]
            extra = {"lazy"} | ({"mask"} if kind == "sql" and "mask" in query else set())
            page = {"field", "offset", "revision"} if "field" in query else set()
            patterns = {"id": r"[a-f0-9]{64}", "thread": r"[a-fA-F0-9-]{36}", "call": r"[A-Za-z0-9_.:-]{1,160}", "thread_id": r"[a-fA-F0-9-]{36}", "call_id": r"[A-Za-z0-9_.:-]{1,160}", "index": r"\d{1,2}", "operation": r"[a-z-]{1,40}", "lazy": "1", "mask": "[01]", "field": "(?:request|response|output|sql|text)", "offset": r"\d{1,12}", "revision": r"[a-f0-9]{64}"}
            if kind == "check":
                patterns["operation"] = r"[^\x00-\x1f\x7f]{1,160}"
            if len(url.query)>1024 or set(query)!=base|extra|page or any(len(items)!=1 or not re.fullmatch(patterns.get(key, r"(?!)"), items[0]) for key, items in query.items()):
                self.reply(400, b"Invalid content page")
                return
            values = {key: items[0] for key, items in query.items()}
            if kind == "sql":
                result = dashboard.sql_detail(values["id"], values.get("mask", "1") == "1", full=True)
            elif kind == "error":
                result = dashboard.error_detail(values["id"], full=True)
            elif kind == "tool":
                result = dashboard.tool_detail(values["thread_id"], values["call_id"], full=True)
            elif kind == "jev":
                result = dashboard.jev_detail(values["thread"], values["call"], int(values["index"]), full=True)
            elif kind == "mcp":
                result = dashboard.mcp_detail(values["thread_id"], values["call_id"], int(values["index"]), full=True)
            elif kind == "check":
                result = dashboard.check_detail(values["thread_id"], values["call_id"], values["operation"], full=True)
            else:
                result = dashboard.git_detail(values["thread_id"], values["call_id"], values["operation"], full=True)
            status = 200 if result is not None else 409
            if result is not None:
                try:
                    result = paged_content(result, values.get("field"), int(values.get("offset", 0)), values.get("revision"))
                except KeyError:
                    status, result = 400, None
                except ValueError:
                    status, result = 409, None
            self.reply(status, json.dumps(result, ensure_ascii=False, allow_nan=False).encode("utf-8"), "application/json; charset=utf-8")

        def do_GET(self):
            if not self.local_request():
                return
            url = urlsplit(self.path)
            detail_routes = {'tool','mcp','git','check','sql','error','jev','context','agent-message'}
            query = parse_qs(url.query,keep_blank_values=True)
            if url.path.startswith('/api/codex/') and url.path.rsplit('/',1)[-1] in detail_routes:
                values = query.get('mask',['1'])
                if len(values)!=1 or values[0] not in ('0','1'):
                    self.reply(400,b'Invalid content mask')
                    return
                masked = values[0]=='1'
                if url.path!='/api/codex/sql':
                    query.pop('mask',None)
                    url = url._replace(query=urlencode(query,doseq=True))
                with mask_payloads(masked):
                    self.get_local(url)
            else:
                self.get_local(url)

        def get_local(self, url):
            if url.path == "/api/events":
                if url.query or not dashboard.event_clients.acquire(blocking=False):
                    self.reply(400 if url.query else 503, b"Event stream unavailable")
                    return
                try:
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream; charset=utf-8")
                    self.send_header("Cache-Control", "no-store")
                    self.send_header("Connection", "close")
                    self.send_header("X-Content-Type-Options", "nosniff")
                    self.end_headers()
                    self.close_connection = True
                    self.wfile.write(b"retry: 3000\n\n")
                    self.wfile.flush()
                    previous = -1
                    while not dashboard.stop.is_set():
                        with dashboard.update_condition:
                            dashboard.update_condition.wait_for(lambda: dashboard.stop.is_set() or dashboard.update_version != previous, timeout=15)
                            current = dashboard.update_version
                        if dashboard.stop.is_set():
                            break
                        event = f"id: {current}\nevent: snapshot\ndata: {{\"version\":{current}}}\n\n" if current != previous else ": heartbeat\n\n"
                        self.wfile.write(event.encode("ascii"))
                        self.wfile.flush()
                        previous = current
                except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError, TimeoutError):
                    self.close_connection = True
                finally:
                    dashboard.event_clients.release()
                return
            if url.path in ('/api/codex/context','/api/codex/agent-message'):
                query = parse_qs(url.query,keep_blank_values=True)
                allowed = {'thread_id','call_id','index','mask'} if url.path.endswith('agent-message') else {'thread_id','call_id','mask'}
                if len(url.query)>256 or set(query)-allowed or 'thread_id' not in query or any(len(values)!=1 for values in query.values()) or query.get('mask',['1'])[0] not in ('0','1') or not re.fullmatch(r'[a-fA-F0-9-]{36}',query['thread_id'][0]) or 'call_id' in query and not re.fullmatch(r'[A-Za-z0-9_.:-]{1,160}',query['call_id'][0]) or 'index' in query and not re.fullmatch(r'\d{1,2}',query['index'][0]):
                    self.reply(400,b'Invalid context query')
                    return
                result = None
                if dashboard.codex and dashboard.observations['codex']:
                    with mask_payloads(query.get('mask',['1' if payload_masking.get() else '0'])[0] == '1'):
                        if url.path.endswith('agent-message') and 'call_id' in query:
                            result = dashboard.codex.agent_message_detail(query['thread_id'][0],query['call_id'][0],int(query.get('index',['0'])[0]))
                        elif url.path.endswith('context'):
                            result = dashboard.codex.context_detail(query['thread_id'][0],query.get('call_id',[None])[0])
                self.reply(200 if result is not None else 409,json.dumps(result,ensure_ascii=False,allow_nan=False).encode('utf-8'),'application/json; charset=utf-8')
                return
            if url.path in ("/api/codex/tool", "/api/codex/mcp", "/api/codex/git", "/api/codex/check", "/api/codex/sql", "/api/codex/error", "/api/codex/jev") and "lazy" in parse_qs(url.query, keep_blank_values=True):
                self.content_request(url)
                return
            if url.path == "/api/logs":
                query = parse_qs(url.query, keep_blank_values=True)
                window = query.get("window", ["all"])[0]
                if window not in WINDOWS or set(query)-{"window"} or len(query.get("window", ["all"])) != 1:
                    self.reply(400, b"Invalid log query")
                    return
                self.reply(200, json.dumps(dashboard.logs(window), ensure_ascii=False, allow_nan=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/sql":
                query = parse_qs(url.query, keep_blank_values=True)
                if query.get("mask", ["1"])[0] not in ("0", "1") or len(url.query)>80 or set(query) not in ({"id"}, {"id", "mask"}) or any(len(items)!=1 for items in query.values()) or not re.fullmatch(r"[a-f0-9]{64}", query["id"][0]):
                    self.reply(400, b"Invalid SQL record")
                    return
                result = dashboard.sql_detail(query["id"][0], query.get("mask", ["1"])[0] == "1")
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/error":
                query = parse_qs(url.query, keep_blank_values=True)
                if len(url.query)>256 or set(query)!={"id"} or any(len(items)!=1 for items in query.values()) or not re.fullmatch(r"[a-f0-9]{64}", query["id"][0]):
                    self.reply(400, b"Invalid error identity")
                    return
                result = dashboard.error_detail(query["id"][0])
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/jev":
                if len(url.query) > 512:
                    self.reply(400, b"Invalid query")
                    return
                query = parse_qs(url.query)
                try:
                    if set(query) != {"thread", "call", "index"} or any(len(item) != 1 for item in query.values()):
                        raise ValueError()
                    thread_id, call_id, index = query["thread"][0], query["call"][0], int(query["index"][0])
                    if not 0 < len(thread_id) <= 160 or not 0 < len(call_id) <= 160 or not 0 <= index < 100:
                        raise ValueError()
                except (ValueError, KeyError):
                    self.reply(400, b"Invalid Jev call")
                    return
                result = dashboard.jev_detail(thread_id, call_id, index)
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/git":
                query = parse_qs(url.query, keep_blank_values=True)
                if len(url.query)>512 or set(query)!={"thread_id", "call_id", "operation"} or any(len(value)!=1 for value in query.values()) or not re.fullmatch(r"[a-fA-F0-9-]{36}", query["thread_id"][0]) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,160}", query["call_id"][0]) or not re.fullmatch(r"[a-z-]{1,40}", query["operation"][0]):
                    self.reply(400, b"Invalid Git event")
                    return
                result = dashboard.git_detail(query["thread_id"][0], query["call_id"][0], query["operation"][0])
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/file-summary":
                query = parse_qs(url.query, keep_blank_values=True)
                if len(url.query)>30 or set(query)!={'window'} or len(query['window'])!=1 or query['window'][0] not in WINDOWS:
                    self.reply(400, b'Invalid file window')
                    return
                result = dashboard.file_summary(query['window'][0])
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode('utf-8'), 'application/json; charset=utf-8')
            elif url.path == "/api/codex/file":
                query = parse_qs(url.query, keep_blank_values=True)
                if len(url.query)>8192 or set(query)!={"thread_id", "call_id", "path"} or any(len(value)!=1 for value in query.values()) or not re.fullmatch(r"[a-fA-F0-9-]{36}", query["thread_id"][0]) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,160}", query["call_id"][0]) or not re.fullmatch(r"[^\x00-\x1f\x7f]{1,2048}", query["path"][0]):
                    self.reply(400, b"Invalid file event")
                    return
                result = dashboard.file_detail(query["thread_id"][0], query["call_id"][0], query["path"][0])
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/worktree":
                if len(url.query) > 64:
                    return self.reply(400, b'{"error":"invalid worktree id"}')
                query = parse_qs(url.query, keep_blank_values=True)
                if set(query) != {"id"} or len(query["id"]) != 1 or not re.fullmatch(r"[a-f0-9]{24}", query["id"][0]):
                    self.reply(400, b"Invalid worktree query")
                    return
                result = dashboard.worktree_detail(query["id"][0])
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path in ("/api/codex/project", "/api/codex/instructions"):
                query = parse_qs(url.query, keep_blank_values=True)
                project_id = query.get("project_id", [None])[0]
                valid_id = project_id is None or bool(re.fullmatch(r"[^\x00-\x1f\x7f/\\]{1,160}", project_id))
                allowed = ({"project_id"},) if url.path.endswith("/project") else ({"scope"}, {"scope", "project_id"})
                scope = query.get("scope", [None])[0]
                valid_scope = url.path.endswith("/project") or scope == "global" and project_id is None or scope == "project" and project_id is not None
                if len(url.query)>1024 or set(query) not in allowed or any(len(value)!=1 for value in query.values()) or not valid_id or not valid_scope:
                    self.reply(400, b"Invalid project query")
                    return
                result = dashboard.project_detail(project_id) if url.path.endswith("/project") else dashboard.instruction_detail(scope, project_id)
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/skill":
                query = parse_qs(url.query, keep_blank_values=True)
                document = query.get("file", [None])[0]
                if len(url.query)>2048 or set(query) not in ({"skill"}, {"skill", "file"}, {"skill", "thread_id", "call_id"}, {"skill", "file", "thread_id", "call_id"}) or any(len(value)!=1 for value in query.values()) or not re.fullmatch(r"[^\x00-\x1f\x7f/\\]{1,160}",query["skill"][0]) or query["skill"][0] in (".","..") or document is not None and (not re.fullmatch(r"[^\x00-\x1f\x7f\\:]{1,512}", document) or document.startswith("/") or any(part in ("", ".", "..") for part in document.split("/"))):
                    self.reply(400, b"Invalid skill")
                    return
                thread_id, call_id = query.get("thread_id", [None])[0], query.get("call_id", [None])[0]
                if thread_id is not None and (not re.fullmatch(r"[a-fA-F0-9-]{36}", thread_id) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,160}", call_id)):
                    self.reply(400, b"Invalid skill event")
                    return
                result = dashboard.skill_detail(query["skill"][0], document, thread_id, call_id)
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/instance":
                self.reply(200, json.dumps({"application": "local-activity-monitor", "home_id": home_id(dashboard.home)}).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/tool":
                query = parse_qs(url.query, keep_blank_values=True)
                if len(url.query)>512 or set(query)!={'thread_id', 'call_id'} or any(len(items)!=1 for items in query.values()) or not re.fullmatch(r'[a-fA-F0-9-]{36}', query['thread_id'][0]) or not re.fullmatch(r'[A-Za-z0-9_.:-]{1,160}', query['call_id'][0]):
                    self.reply(400, b'Invalid tool call')
                    return
                result = dashboard.tool_detail(query['thread_id'][0], query['call_id'][0])
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode('utf-8'), 'application/json; charset=utf-8')
            elif url.path == "/api/codex/mcp":
                query = parse_qs(url.query, keep_blank_values=True)
                if len(url.query)>512 or set(query)!={"thread_id", "call_id", "index"} or any(len(items)!=1 for items in query.values()) or not re.fullmatch(r"[a-fA-F0-9-]{36}", query["thread_id"][0]) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,160}", query["call_id"][0]) or not re.fullmatch(r"\d{1,2}", query["index"][0]):
                    self.reply(400, b"Invalid MCP call")
                    return
                result = dashboard.mcp_detail(query["thread_id"][0], query["call_id"][0], int(query["index"][0]))
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/mcp/files":
                query = parse_qs(url.query, keep_blank_values=True)
                if len(url.query)>256 or set(query) not in ({"server"}, {"server", "document"}) or any(len(items)!=1 for items in query.values()) or not SOURCE.fullmatch(query["server"][0]) or "document" in query and not re.fullmatch(r"[a-f0-9]{32}", query["document"][0]):
                    self.reply(400, b"Invalid MCP document")
                    return
                result = dashboard.mcp_documents(query["server"][0], query.get("document", [None])[0])
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/history":
                query = parse_qs(url.query, keep_blank_values=True)
                try:
                    if len(url.query)>512 or set(query)-{'namespace', 'section', 'limit', 'offset', 'since', 'cursor'} or any(len(values)!=1 for values in query.values()):
                        raise ValueError('Invalid history query')
                    namespace, section = query.get('namespace', ['activity'])[0], query.get('section', [''])[0]
                    stores = {'activity': dashboard.activity_history.store, 'errors': dashboard.error_history.store}
                    if dashboard.codex:
                        stores['threads'] = dashboard.codex.thread_state.store
                    observation = {'sql': 'sqlite', 'web': 'web', 'mcp': 'mcp', 'events': 'errors', 'skills': 'skills', 'entries': 'codex'}.get(section)
                    if namespace not in stores or not observation or not dashboard.observations.get(observation):
                        raise ValueError('Unavailable history source')
                    excluded = [key for key, enabled in dashboard.mcp_sources.items() if enabled is False] if section=='mcp' else []
                    result = stores[namespace].page(section, limit=int(query.get('limit', ['100'])[0]), offset=int(query.get('offset', ['0'])[0]), since=query.get('since', [None])[0], cursor=query.get('cursor', [None])[0], excluded_sources=excluded)
                except (ValueError, TypeError):
                    self.reply(400, b'Invalid history query')
                    return
                except (OSError, sqlite3.Error):
                    self.reply(503, b'History temporarily unavailable')
                    return
                self.reply(200, json.dumps(result, ensure_ascii=False).encode('utf-8'), 'application/json; charset=utf-8')
            elif url.path == "/api/activity" and not url.query:
                self.reply(200, json.dumps(dashboard.activity_status()).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/snapshot":
                if len(url.query) > 128:
                    self.reply(400, b"Invalid query")
                    return
                query = parse_qs(url.query)
                window = query.get("window", ["24h"])[0]
                if window not in WINDOWS or set(query)-{"window"}:
                    self.reply(400, b"Invalid window")
                    return
                raw = json.dumps(dashboard.snapshot(window), ensure_ascii=False, allow_nan=False).encode("utf-8")
                self.reply(200, raw, "application/json; charset=utf-8")
            elif url.path in assets:
                file, mime = assets[url.path]
                root = frontend.root
                path = library.path(file) if file in ("workbench-ui.js", "workbench-ui.css") else root/file
                content, script_hash, style_hash = path.read_bytes(), None, None
                if file == "index.html":
                    # Keep startup assets in one response, with exact CSP hashes.
                    script = (library.path("workbench-ui.js").read_text(encoding="utf-8")+";\n"+(root/"app.js").read_text(encoding="utf-8")).encode("utf-8").replace(b"</", b"<\\/")
                    style = (library.path("workbench-ui.css").read_text(encoding="utf-8")+"\n"+(root/"style.css").read_text(encoding="utf-8")).encode("utf-8")
                    content = content.replace(b"</body>", b"<script>"+script+b"</script>\n</body>")
                    content = content.replace(b'<link rel="stylesheet" href="/style.css">', b"<style>"+style+b"</style>")
                    content = content.replace(b'<link rel="stylesheet" href="/workbench-ui.css">', b"")
                    script_hash = base64.b64encode(hashlib.sha256(script).digest()).decode("ascii")
                    style_hash = base64.b64encode(hashlib.sha256(style).digest()).decode("ascii")
                self.reply(200, content, mime, script_hash=script_hash, style_hash=style_hash)
            else:
                self.reply(404, b"Not found")

        def do_POST(self):
            if not self.local_request():
                return
            if self.path not in ("/api/jev/recording", "/api/settings", "/api/refresh", "/api/mcp/file", "/api/diagnostics"):
                self.reply(405, b"Unsupported operation")
                return
            if self.headers.get("Content-Type", "").split(";", 1)[0].strip() != "application/json" or self.headers.get("Transfer-Encoding"):
                self.reply(400, b"Invalid content type")
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= (262144 if self.path in ("/api/settings", "/api/mcp/file") else 2048 if self.path == "/api/diagnostics" else 512):
                    raise ValueError()
                value = json.loads(self.rfile.read(length))
                if self.path == "/api/jev/recording" and (not isinstance(value, dict) or set(value) != {"enabled"} or type(value["enabled"]) is not bool):
                    raise ValueError()
                if self.path == "/api/refresh" and value != {}:
                    raise ValueError()
                if self.path == "/api/mcp/file" and (not isinstance(value, dict) or set(value)!={"server", "document", "sha256", "text"} or not isinstance(value["server"], str) or not SOURCE.fullmatch(value["server"]) or not isinstance(value["document"], str) or not re.fullmatch(r"[a-f0-9]{32}", value["document"]) or not isinstance(value["sha256"], str) or not re.fullmatch(r"[a-f0-9]{64}", value["sha256"]) or not isinstance(value["text"], str)):
                    raise ValueError()
            except (ValueError, TypeError):
                self.reply(400, b"Invalid recording setting")
                return
            if self.path == "/api/diagnostics":
                try:
                    recorded = dashboard.monitor.frontend_failed(value)
                except ValueError:
                    self.reply(400, b"Invalid diagnostic metadata")
                    return
                self.reply(202, json.dumps({"recorded": recorded}).encode("utf-8"), "application/json; charset=utf-8")
                return
            try:
                result = dashboard.save_mcp_document(value["server"], value["document"], value["text"], value["sha256"]) if self.path == "/api/mcp/file" else dashboard.resume_refresh() if self.path == "/api/refresh" else dashboard.set_jev_recording(value["enabled"]) if self.path == "/api/jev/recording" else dashboard.set_settings(value)
                code = 200
            except FileExistsError:
                result, code = {"error": "檔案已變更, 請重新讀取後再儲存"}, 409
            except ValueError:
                result, code = {"error": "檔案無法編輯或內容格式無效"} if self.path == "/api/mcp/file" else {"error": "既有 Jev 設定無效, 請先檢查設定檔"} if self.path == "/api/jev/recording" else {"error": "觀察設定的格式或數值無效"}, 409
            except OSError:
                result, code = {"error": "無法儲存檔案, 請檢查存取權限"} if self.path == "/api/mcp/file" else {"error": "無法寫入本機 Jev 設定, 請檢查存取權限"}, 500
            self.reply(code, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")

    return Handler


class LoopbackHTTPServer(ThreadingHTTPServer):
    def server_bind(self):
        # The service uses a literal loopback address and needs no reverse DNS.
        TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


def main(argv=None, watch_stdin=False):
    with cli_lifetime():
        return serve(argv, watch_stdin)


def serve(argv=None, watch_stdin=False):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex-home", type=Path, default=Path(os.environ.get("CODEX_HOME") or Path.home()/".codex"))
    parser.add_argument("--port", type=int, default=8787)
    parser.add_argument("--codex", action="store_true", help="Opt in to read recent Codex session metadata")
    parser.add_argument("--max-files", type=int, default=20)
    parser.add_argument("--open", action="store_true")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--enable-jev", action="store_true", help="Opt in to persistent Jev metadata recording")
    group.add_argument("--disable-jev", action="store_true")
    parser.add_argument("--database", type=Path)
    parser.add_argument("--configure-only", action="store_true")
    args = parser.parse_args(argv)
    if not 0 <= args.port <= 65535 or not 1 <= args.max_files <= 5000:
        parser.error("Invalid port or max-files")
    home = args.codex_home.expanduser().resolve()
    if args.enable_jev or args.disable_jev:
        print(json.dumps({"jev_recording": configure(home, args.enable_jev, args.database), "reload_existing_jev_processes": True}))
    if args.configure_only:
        return 0
    existing = existing_instance(args.port, home)
    if existing:
        print(json.dumps({"status": "already_running", "url": existing}), flush=True)
        if args.open:
            webbrowser.open(existing)
        return 0
    try:
        load_ui_assets()
        load_frontend_assets()
    except (OSError, ValueError, RuntimeError) as error:
        print(str(error), file=sys.stderr)
        return 1
    try:
        server = LoopbackHTTPServer(("127.0.0.1", args.port), BaseHTTPRequestHandler)
    except OSError:
        print("Port is already in use. Keep the existing monitor open or choose another --port.", file=sys.stderr)
        return 1
    dashboard = Dashboard(home, args.codex, args.max_files)
    dashboard.preload()
    server.RequestHandlerClass = handler(dashboard, server.server_port)
    dashboard.thread.start()
    url = f"http://127.0.0.1:{server.server_port}/"
    print(json.dumps({"status": "listening", "url": url, "codex_metadata": args.codex}), flush=True)
    if args.open:
        webbrowser.open(url)
    if watch_stdin:
        def watch_control():
            try:
                command = sys.stdin.readline()
                if not command or command.strip() == "restart":
                    dashboard.monitor.event("restart_requested" if command else "launcher_closed")
                    server.shutdown()
            except OSError:
                pass
        threading.Thread(target=watch_control, name="watch-control", daemon=True).start()
    try:
        server.serve_forever(poll_interval=.5)
    except KeyboardInterrupt:
        pass
    finally:
        dashboard.stop.set()
        dashboard.notify_update()
        dashboard.monitor.event("stopped")
        server.server_close()
        dashboard.thread.join(timeout=12)
    return 0
