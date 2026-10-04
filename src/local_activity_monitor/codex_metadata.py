"""Select local Codex catalog fields without exposing other app state."""
from contextlib import closing
from datetime import datetime, timezone
import json
import re
import sqlite3


def text(value, limit=512):
    return value.strip()[:limit] if isinstance(value, str) and value.strip() else None


def date(value):
    try:
        if type(value) in (int, float):
            return datetime.fromtimestamp(value, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
    except (ValueError, OverflowError, OSError):
        pass
    return None


def execution_metadata(values):
    """Project selected execution settings, without permission roots or raw policies."""
    result = {}
    for key in ("model_provider", "cli_version", "originator", "history_mode", "approval_policy", "agent_nickname", "agent_role", "service_tier", "reasoning_summary", "approvals_reviewer"):
        alias = "approval_mode" if key == "approval_policy" else "model_provider_id" if key == "model_provider" else key
        value = values.get(key, values.get(alias))
        if isinstance(value, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,160}", value):
            result[key] = value
    for key, source in (("sandbox_mode", "sandbox_policy"), ("collaboration_mode", "collaboration_mode")):
        value = values.get(source)
        if isinstance(value, str) and value.startswith("{") and len(value) <= 4096:
            try:
                value = json.loads(value)
            except (ValueError, RecursionError):
                value = None
        if isinstance(value, dict):
            value = value.get("type" if key == "sandbox_mode" else "mode")
        if isinstance(value, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", value):
            result[key] = value
    window = values.get("context_window")
    if type(window) is int and 0 < window <= 2**63-1:
        result["context_window"] = window
    branch, commit = values.get("git_branch"), values.get("git_sha")
    if isinstance(branch, str) and re.fullmatch(r"[a-zA-Z0-9_./-]{1,160}", branch):
        result["git_branch"] = branch
    if isinstance(commit, str) and re.fullmatch(r"[a-fA-F0-9]{7,64}", commit):
        result["git_commit"] = commit
    parent = values.get("parent_thread_id")
    if isinstance(parent, str) and re.fullmatch(r"[a-fA-F0-9-]{36}", parent):
        result["parent_thread_id"] = parent
    return result


def read_metadata(home, identities):
    entries = {}
    try:
        path = home/"sqlite/codex-dev.db"
        with closing(sqlite3.connect(path.as_uri()+"?mode=ro", uri=True, timeout=.08)) as db:
            db.execute("PRAGMA query_only=ON")
            columns = {row[1] for row in db.execute("PRAGMA table_info(local_thread_catalog)")}
            required = ("host_id", "thread_id", "display_title", "source_created_at", "source_updated_at", "source_kind", "thread_source", "project_id")
            optional = [key for key in ("model", "reasoning_effort", "model_provider", "chatgpt_async_status") if key in columns]
            wanted = (*required, *optional)
            if set(required) <= columns:
                for row in db.execute("SELECT "+",".join(wanted)+" FROM local_thread_catalog ORDER BY source_updated_at DESC LIMIT 2000"):
                    host, identity, title, created, updated, source, trigger, project = row[:8]
                    if not isinstance(identity, str) or len(identity) > 160:
                        continue
                    environment = "local" if host == "local" else "cloud" if host == "durable" or isinstance(host, str) and host.startswith("chatgpt:") else "remote" if isinstance(host, str) and host.startswith("remote-control:") else "unknown"
                    current = entries.get(identity)
                    if current and (current.get("updated_at") or "") >= (date(updated) or ""):
                        continue
                    entries[identity] = {"thread_name": text(title), "created_at": date(created), "updated_at": date(updated), "activity_type": "chat" if source == "chatgpt" else "codex" if source in ("vscode", "cli", "exec") else "unknown", "environment": environment, "project_id": text(project, 160), "project_scope": "project" if text(project, 160) else "none", "trigger": trigger if trigger in ("dot", "orbit", "automation", "schedule", "heartbeat", "subagent", "guardian_review", "user") else "unknown"}
                    values = dict(zip(optional, row[8:]))
                    for key in ("model", "reasoning_effort"):
                        item = values.get(key)
                        if isinstance(item, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,160}", item):
                            entries[identity][key] = item
                    entries[identity]["execution"] = execution_metadata(values)
                    status = values.get("chatgpt_async_status")
                    if isinstance(status, str) and re.fullmatch(r"[a-z][a-z_]{0,39}", status):
                        entries[identity]["status"] = status
            try:
                for identity, kind in db.execute("SELECT r.thread_id,a.kind FROM automation_runs r JOIN automations a ON a.id=r.automation_id"):
                    if identity in entries:
                        entries[identity]["trigger"] = "heartbeat" if kind == "heartbeat" else "schedule"
                for identity, kind in db.execute("SELECT target_thread_id,kind FROM automations WHERE target_thread_id IS NOT NULL AND status='ACTIVE'"):
                    if identity in entries:
                        entries[identity]["has_schedule"] = True
            except sqlite3.Error:
                pass
    except (OSError, sqlite3.Error):
        pass
    databases = []
    try:
        for path in home.glob("state_*.sqlite"):
            match = re.fullmatch(r"state_(\d+)\.sqlite", path.name)
            if match:
                databases.append((int(match[1]), path))
        selected = set(entries) | set(identities)
        for _, path in sorted(databases, reverse=True):
            try:
                with closing(sqlite3.connect(path.as_uri()+"?mode=ro", uri=True, timeout=.08)) as db:
                    db.execute("PRAGMA query_only=ON")
                    columns = {row[1] for row in db.execute("PRAGMA table_info(threads)")}
                    wanted = [key for key in ("id", "title", "model", "reasoning_effort", "originator", "thread_source", "model_provider", "cli_version", "approval_mode", "sandbox_policy", "git_branch", "git_sha", "agent_nickname", "agent_role", "history_mode") if key in columns]
                    if not {"id", "title"} <= set(wanted):
                        continue
                    for start in range(0, len(selected), 400):
                        batch = sorted(selected)[start:start+400]
                        placeholders = ",".join("?" for _ in batch)
                        for row in db.execute("SELECT "+",".join(wanted)+f" FROM threads WHERE id IN ({placeholders})", batch):
                            values = dict(zip(wanted, row))
                            entry = entries.setdefault(values["id"], {})
                            if not entry.get("thread_name") and text(values.get("title")):
                                entry["thread_name"] = text(values["title"])
                            if text(values.get("model"), 160):
                                entry["model"] = text(values["model"], 160)
                            effort = values.get("reasoning_effort")
                            if isinstance(effort, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", effort):
                                entry["reasoning_effort"] = effort
                            entry["execution"] = execution_metadata(values)
                            if values.get("originator") == "codex_work_desktop":
                                entry["activity_type"] = "work"
                            elif values.get("originator") == "Codex Desktop":
                                entry["activity_type"] = "codex"
                            if values.get("thread_source") in ("user", "subagent", "guardian_review"):
                                entry["trigger"] = values["thread_source"]
                break
            except sqlite3.Error:
                continue
    except OSError:
        pass
    try:
        path = home/"session_index.jsonl"
        with path.open("rb") as stream:
            stream.seek(0, 2)
            offset = max(0, stream.tell()-1024*1024)
            stream.seek(offset)
            if offset:
                stream.readline()
            indexed = {}
            for raw in stream:
                try:
                    value = json.loads(raw)
                    if isinstance(value, dict) and value.get("id") in identities and text(value.get("thread_name")):
                        indexed[value["id"]] = text(value["thread_name"])
                except (ValueError, TypeError, RecursionError):
                    continue
            for identity, title in indexed.items():
                entry = entries.setdefault(identity, {})
                if not entry.get("thread_name"):
                    entry["thread_name"] = title
    except OSError:
        pass
    try:
        path = home/".codex-global-state.json"
        if path.stat().st_size <= 16*1024*1024:
            state = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(state, dict):
                projects = state.get("local-projects", {})
                assignments = state.get("thread-project-assignments", {})
                for identity, entry in entries.items():
                    assignment = assignments.get(identity) if isinstance(assignments, dict) else None
                    if isinstance(assignment, dict) and text(assignment.get("projectId"), 160):
                        entry.update(project_id=text(assignment["projectId"], 160), project_scope="project")
                    project = projects.get(entry.get("project_id")) if isinstance(projects, dict) else None
                    if isinstance(project, dict):
                        entry["project_name"] = text(project.get("name"))
                atom = state.get("electron-persisted-atom-state", {})
                outputs = atom.get("orbit-outputs-v1", []) if isinstance(atom, dict) else []
                if isinstance(outputs, list):
                    for output in outputs:
                        if isinstance(output, dict) and isinstance(output.get("threadId"), str) and output["threadId"] in entries:
                            entries[output["threadId"]]["trigger"] = "dot"
    except (OSError, ValueError, RecursionError):
        pass
    return entries
