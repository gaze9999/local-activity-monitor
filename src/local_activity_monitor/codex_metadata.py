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
    path = values.get("agent_path")
    if isinstance(path, str) and re.fullmatch(r"/root(?:/[A-Za-z0-9_.-]{1,160}){0,12}", path):
        result["agent_path"] = path
    return result


def spawn_metadata(value):
    """Read the selected subagent source descriptor without retaining raw source."""
    if isinstance(value, str) and len(value) <= 4096:
        try:
            value = json.loads(value)
        except (ValueError, RecursionError):
            return {}
    subagent = value.get("subagent") if isinstance(value, dict) else None
    spawn = subagent.get("thread_spawn") if isinstance(subagent, dict) else None
    return execution_metadata(spawn) if isinstance(spawn, dict) else {}


CATALOG_LIMIT = 2000
INDEX_TAIL_LIMIT = 1024*1024
APP_STATE_LIMIT = 16*1024*1024
PROJECT_LIMIT = 2000
PROJECT_ROOT_LIMIT = 5000
SUBAGENT_LIMIT = 500


def read_metadata(home, identities, dots=None, source_info=None, project_details=None):
    entries = {}
    def report(path, health, fields=(), **limits):
        if source_info is not None:
            source_info[str(path)] = {"name": path.name, "location": str(path), "health": health, "fields": list(fields), **limits}
    def failed(path):
        try:
            health = "unavailable" if path.is_file() else "missing"
        except OSError:
            health = "unavailable"
        report(path, health)
    try:
        path = home/"sqlite/codex-dev.db"
        with closing(sqlite3.connect(path.as_uri()+"?mode=ro", uri=True, timeout=.08)) as db:
            db.execute("PRAGMA query_only=ON")
            columns = {row[1] for row in db.execute("PRAGMA table_info(local_thread_catalog)")}
            required = ("host_id", "thread_id", "display_title", "source_created_at", "source_updated_at", "source_kind", "thread_source", "project_id")
            optional = [key for key in ("model", "reasoning_effort", "model_provider", "chatgpt_async_status", "archived") if key in columns]
            wanted = (*required, *optional)
            if set(required) <= columns:
                rows_read = 0
                for row in db.execute("SELECT "+",".join(wanted)+f" FROM local_thread_catalog ORDER BY source_updated_at DESC LIMIT {CATALOG_LIMIT}"):
                    rows_read += 1
                    host, identity, title, created, updated, source, trigger, project = row[:8]
                    if not isinstance(identity, str) or len(identity) > 160:
                        continue
                    environment = "local" if host == "local" else "cloud" if host == "durable" or isinstance(host, str) and host.startswith("chatgpt:") else "remote" if isinstance(host, str) and host.startswith("remote-control:") else "unknown"
                    current = entries.get(identity)
                    if current and (current.get("updated_at") or "") >= (date(updated) or ""):
                        continue
                    entries[identity] = {"thread_name": text(title), "created_at": date(created), "updated_at": date(updated), "activity_type": "chat" if source == "chatgpt" else "codex" if source in ("vscode", "cli", "exec") else "unknown", "environment": environment, "project_id": text(project, 160), "project_scope": "project" if text(project, 160) else "none", "trigger": trigger if trigger in ("dot", "orbit", "automation", "schedule", "heartbeat", "subagent", "guardian_review", "user") else "unknown"}
                    values = dict(zip(optional, row[8:]))
                    entries[identity]["_host_id"] = host
                    if type(values.get("archived")) is int and values["archived"] in (0, 1):
                        entries[identity]["archived"] = bool(values["archived"])
                    for key in ("model", "reasoning_effort"):
                        item = values.get(key)
                        if isinstance(item, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,160}", item):
                            entries[identity][key] = item
                    entries[identity]["execution"] = execution_metadata(values)
                    status = values.get("chatgpt_async_status")
                    if isinstance(status, str) and re.fullmatch(r"[a-z][a-z_]{0,39}", status):
                        entries[identity]["status"] = status
                report(path, "ok", wanted, rows_read=rows_read, row_limit=CATALOG_LIMIT)
            else:
                report(path, "unsupported")
            selected = sorted(entries)
            queries = []
            specifications = (
                ("automation_runs", ("automation_runs.thread_id", "automations.kind"),
                 "SELECT r.thread_id,MAX(CASE WHEN a.kind='heartbeat' THEN 1 ELSE 0 END) FROM automation_runs r JOIN automations a ON a.id=r.automation_id WHERE r.thread_id IN ({}) GROUP BY r.thread_id"),
                ("automations", ("target_thread_id", "status"),
                 "SELECT target_thread_id FROM automations WHERE status='ACTIVE' AND target_thread_id IN ({}) GROUP BY target_thread_id"),
            )
            for name, fields, query in specifications:
                query_info = {"name": name, "health": "ok", "fields": list(fields), "rows_read": 0, "selected_threads": len(selected), "row_limit": len(selected)}
                try:
                    for start in range(0, len(selected), 400):
                        batch = selected[start:start+400]
                        for row in db.execute(query.format(",".join("?" for _ in batch)), batch):
                            query_info["rows_read"] += 1
                            if name == "automation_runs":
                                entries[row[0]]["trigger"] = "heartbeat" if row[1] else "schedule"
                            else:
                                entries[row[0]]["has_schedule"] = True
                except sqlite3.Error:
                    query_info["health"] = "unsupported"
                queries.append(query_info)
            if source_info is not None:
                source_info[str(path)]["queries"] = queries
    except (OSError, sqlite3.Error):
        failed(home/"sqlite/codex-dev.db")
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
                    wanted = [key for key in ("id", "title", "model", "reasoning_effort", "originator", "thread_source", "model_provider", "cli_version", "approval_mode", "sandbox_policy", "git_branch", "git_sha", "agent_nickname", "agent_role", "agent_path", "source", "history_mode", "archived", "project_id", "created_at", "updated_at", "created_at_ms", "updated_at_ms") if key in columns]
                    if not {"id", "title"} <= set(wanted):
                        report(path, "unsupported")
                        continue
                    edges = {}
                    edge_columns = {row[1] for row in db.execute("PRAGMA table_info(thread_spawn_edges)")}
                    if {"parent_thread_id", "child_thread_id"} <= edge_columns:
                        # Select children of known parents only, including bounded descendants.
                        pending, visited = set(selected), set()
                        while pending and len(edges) < SUBAGENT_LIMIT:
                            batch = sorted(pending-visited)[:400]
                            if not batch:
                                break
                            visited.update(batch)
                            pending.difference_update(batch)
                            placeholders = ",".join("?" for _ in batch)
                            for parent, child in db.execute(f"SELECT parent_thread_id,child_thread_id FROM thread_spawn_edges WHERE parent_thread_id IN ({placeholders}) ORDER BY child_thread_id LIMIT {SUBAGENT_LIMIT-len(edges)}", batch):
                                if not all(isinstance(value, str) and re.fullmatch(r"[a-fA-F0-9-]{36}", value) for value in (parent, child)):
                                    continue
                                edges[child] = parent
                                if child not in visited:
                                    pending.add(child)
                        selected.update(edges)
                    rows_read = 0
                    for start in range(0, len(selected), 400):
                        batch = sorted(selected)[start:start+400]
                        placeholders = ",".join("?" for _ in batch)
                        for row in db.execute("SELECT "+",".join(wanted)+f" FROM threads WHERE id IN ({placeholders})", batch):
                            rows_read += 1
                            values = dict(zip(wanted, row))
                            entry = entries.setdefault(values["id"], {})
                            if type(values.get("archived")) is int and values["archived"] in (0, 1):
                                entry["archived"] = bool(values["archived"])
                            if text(values.get("project_id"), 160):
                                entry.update(project_id=text(values["project_id"], 160), project_scope="project")
                            if not entry.get("thread_name") and text(values.get("title")):
                                entry["thread_name"] = text(values["title"])
                            if text(values.get("model"), 160):
                                entry["model"] = text(values["model"], 160)
                            effort = values.get("reasoning_effort")
                            if isinstance(effort, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", effort):
                                entry["reasoning_effort"] = effort
                            entry["execution"] = spawn_metadata(values.get("source")) | execution_metadata(values)
                            if values["id"] in edges:
                                entry["execution"]["parent_thread_id"] = edges[values["id"]]
                                entry["trigger"] = "subagent"
                            for field in ("created_at", "updated_at"):
                                milliseconds = values.get(field+"_ms")
                                when = date(milliseconds/1000) if type(milliseconds) in (int, float) else date(values.get(field))
                                if when and not entry.get(field):
                                    entry[field] = when
                            if values.get("originator") == "codex_work_desktop":
                                entry["activity_type"] = "work"
                            elif values.get("originator") == "Codex Desktop":
                                entry["activity_type"] = "codex"
                            if values.get("thread_source") in ("user", "subagent", "guardian_review"):
                                entry["trigger"] = values["thread_source"]
                    project_fields = []
                    project_rows = root_rows = 0
                    if project_details is not None:
                        project_columns = {row[1] for row in db.execute("PRAGMA table_info(projects)")}
                        root_columns = {row[1] for row in db.execute("PRAGMA table_info(project_roots)")}
                        if {"id", "name"} <= project_columns:
                            project_wanted = [key for key in ("id", "name", "created_at_ms", "updated_at_ms") if key in project_columns]
                            order = " ORDER BY updated_at_ms DESC" if "updated_at_ms" in project_columns else " ORDER BY id"
                            for project_row in db.execute("SELECT "+",".join(project_wanted)+" FROM projects"+order+f" LIMIT {PROJECT_LIMIT}"):
                                project_values = dict(zip(project_wanted, project_row))
                                project_id, project_name = project_values["id"], project_values["name"]
                                if text(project_id, 160):
                                    project_details[project_id] = {"id": project_id, "name": text(project_name), "source": "projects", "folders": []}
                                    for key in ("created_at", "updated_at"):
                                        value = project_values.get(key+"_ms")
                                        if type(value) in (int, float):
                                            project_details[project_id][key] = date(value/1000)
                                project_rows += 1
                            project_fields.extend("projects."+key for key in project_wanted)
                        project_ids = sorted(set(project_details) | {entry["project_id"] for entry in entries.values() if entry.get("project_id")})
                        for start in range(0, len(project_ids), 400):
                            batch = project_ids[start:start+400]
                            placeholders = ",".join("?" for _ in batch)
                            if {"project_id", "path"} <= root_columns and root_rows < PROJECT_ROOT_LIMIT:
                                for project_id, root in db.execute(f"SELECT project_id,path FROM project_roots WHERE project_id IN ({placeholders}) LIMIT {PROJECT_ROOT_LIMIT-root_rows}", batch):
                                    root_rows += 1
                                    detail = project_details.setdefault(project_id, {"id": project_id, "name": None, "source": "project_roots", "folders": []})
                                    if text(root, 4096) and len(detail["folders"]) < 32:
                                        detail["folders"].append(text(root, 4096))
                                project_fields.extend(("project_roots.project_id", "project_roots.path"))
                        for entry in entries.values():
                            detail = project_details.get(entry.get("project_id"), {})
                            if detail.get("name"):
                                entry["project_name"] = detail["name"]
                    edge_fields = ("thread_spawn_edges.parent_thread_id", "thread_spawn_edges.child_thread_id") if {"parent_thread_id", "child_thread_id"} <= edge_columns else ()
                    report(path, "ok", (*wanted, *edge_fields, *sorted(set(project_fields))), rows_read=rows_read, selected_threads=len(selected), subagent_rows=len(edges), subagent_limit=SUBAGENT_LIMIT, project_rows=project_rows, project_root_rows=root_rows, project_limit=PROJECT_LIMIT, project_root_limit=PROJECT_ROOT_LIMIT)
                break
            except sqlite3.Error:
                failed(path)
                continue
    except OSError:
        pass
    try:
        path = home/"session_index.jsonl"
        with path.open("rb") as stream:
            stream.seek(0, 2)
            offset = max(0, stream.tell()-INDEX_TAIL_LIMIT)
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
            report(path, "ok", ("id", "thread_name"), tail_bytes=INDEX_TAIL_LIMIT, matched_threads=len(indexed))
    except OSError:
        failed(home/"session_index.jsonl")
    try:
        path = home/".codex-global-state.json"
        if path.stat().st_size <= APP_STATE_LIMIT:
            state = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(state, dict):
                projects = state.get("local-projects", {})
                assignments = state.get("thread-project-assignments", {})
                mappings = state.get("app-server-project-id-by-legacy-project-id-by-host", {})
                legacy_project_rows = 0
                def mapped_project(project_id, host="local"):
                    mapping = mappings.get(host) if isinstance(mappings, dict) else None
                    target = mapping.get(project_id) if isinstance(mapping, dict) else None
                    return target if isinstance(target, str) and project_details is not None and project_details.get(target, {}).get("source") == "projects" else project_id
                if project_details is not None and isinstance(projects, dict):
                    for legacy_id, project in list(projects.items())[:PROJECT_LIMIT]:
                        if not text(legacy_id, 160) or not isinstance(project, dict):
                            continue
                        legacy_project_rows += 1
                        project_id = mapped_project(legacy_id)
                        detail = project_details.setdefault(project_id, {"id": project_id})
                        if project_id == legacy_id and detail.get("source") != "projects":
                            detail.update(name=text(project.get("name")), source="local-projects")
                        elif not detail.get("name"):
                            detail["name"] = text(project.get("name"))
                        roots = project.get("rootPaths")
                        if isinstance(roots, list) and not detail.get("folders"):
                            detail["folders"] = [text(root, 4096) for root in roots[:32] if text(root, 4096)]
                for identity, entry in entries.items():
                    assignment = assignments.get(identity) if isinstance(assignments, dict) else None
                    if isinstance(assignment, dict) and text(assignment.get("projectId"), 160):
                        entry.update(project_id=mapped_project(text(assignment["projectId"], 160), entry.get("_host_id", "local")), project_scope="project")
                        kind = assignment.get("projectKind")
                        if project_details is not None and isinstance(kind, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", kind):
                            project_details.setdefault(entry["project_id"], {"id": entry["project_id"], "name": None, "folders": []})["kind"] = kind
                        origin = assignment.get("projectOrigin")
                        if project_details is not None and isinstance(origin, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", origin):
                            project_details.setdefault(entry["project_id"], {"id": entry["project_id"], "name": None, "folders": []})["origin"] = origin
                    project = projects.get(assignment.get("projectId") if isinstance(assignment, dict) else entry.get("project_id")) if isinstance(projects, dict) else None
                    if isinstance(project, dict):
                        known = project_details.get(entry.get("project_id"), {}) if project_details is not None else {}
                        entry["project_name"] = known.get("name") or text(project.get("name"))
                        if project_details is not None:
                            detail = project_details.setdefault(entry["project_id"], {"id": entry["project_id"]})
                            if detail.get("source") != "projects":
                                detail.update(name=text(project.get("name")), source="local-projects")
                            roots = project.get("rootPaths")
                            if isinstance(roots, list) and not detail.get("folders"):
                                detail["folders"] = [text(root, 4096) for root in roots[:32] if text(root, 4096)]
                atom = state.get("electron-persisted-atom-state", {})
                outputs = atom.get("orbit-outputs-v1") if isinstance(atom, dict) else None
                if dots is not None:
                    snapshots = atom.get("orbit-activity-snapshots-v1") if isinstance(atom, dict) else None
                    dots.update(events=[] if isinstance(outputs, list) else None, activity_items=sum(len(item["data"]) for item in snapshots[:2000] if isinstance(item, dict) and isinstance(item.get("data"), list)) if isinstance(snapshots, list) else None)
                if isinstance(outputs, list):
                    seen = set()
                    for output in outputs[:2000]:
                        if dots is not None and isinstance(output, dict) and isinstance(output.get("threadId"), str) and re.fullmatch(r"[a-fA-F0-9-]{36}", output["threadId"]):
                            artifact = output.get("artifact")
                            if isinstance(artifact, dict):
                                produced = artifact.get("producedAtMs")
                                when = date(produced/1000) if type(produced) in (int, float) else None
                                kind = artifact.get("type")
                                kind = kind if isinstance(kind, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", kind) else None
                                identity = (output["threadId"], output.get("turnId") if isinstance(output.get("turnId"), str) else None, when, kind)
                                if identity not in seen:
                                    seen.add(identity)
                                    dots["events"].append({"thread_id": output["threadId"], "timestamp": when, "artifact_type": kind})
                        if isinstance(output, dict) and isinstance(output.get("threadId"), str) and output["threadId"] in entries:
                            entries[output["threadId"]]["trigger"] = "dot"
                    if dots is not None:
                        dots["_retained_events"] = dots["events"]
                        dots["events"] = sorted(dots["events"], key=lambda item:item["timestamp"] or "", reverse=True)[:500]
                report(path, "ok", ("project_name", "project_membership", "project_kind", "project_origin", "project_root_metadata", "legacy_project_id_mapping", "dot_thread_id", "artifact_type", "produced_at"), byte_limit=APP_STATE_LIMIT, project_limit=PROJECT_LIMIT, loaded_projects=legacy_project_rows)
            else:
                report(path, "unsupported", byte_limit=APP_STATE_LIMIT)
        else:
            report(path, "oversized", byte_limit=APP_STATE_LIMIT)
    except (ValueError, RecursionError):
        report(home/".codex-global-state.json", "unsupported", byte_limit=APP_STATE_LIMIT)
    except OSError:
        failed(home/".codex-global-state.json")
    return entries
