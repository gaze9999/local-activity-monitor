"""Discover MCP sources and retain bounded operation metadata, never raw payloads."""
from collections import Counter
import json
import ipaddress
import re
from urllib.parse import urlsplit, urlunsplit
try:
    import tomllib
except ImportError:
    tomllib = None

SOURCE = re.compile(r"[A-Za-z0-9_.-]{1,80}\Z")
CATEGORIES = {"documents": "文件 / OCR", "workspace": "環境 / 驗證", "runtime": "Runtime / 瀏覽器", "workflow": "Codex 工作流程", "review": "Review / Security", "deployment": "部署", "apps": "App / 成果", "jev": "Jev", "web": "網路參考", "other": "其他 MCP"}
PRESETS = {"local_documents": "documents", "workspace_inspection": "workspace", "node_repl": "runtime", "cua_repl": "runtime", "codex_app": "workflow", "code_review": "review", "codex_security": "review", "jev": "jev", "web": "web", "vercel": "deployment", "sites": "deployment", "figma": "apps", "notion": "apps", "pages": "apps", "work_pets": "apps"}
PLUGIN_SOURCES = {"codex-app-tools": "codex_app", "code-review": "code_review", "codex-security": "codex_security", "unified-computer-use": "cua_repl"}
STATUSES = {"ok", "success", "completed", "ready", "installed", "extracted", "written", "preview", "unchanged", "partial", "passed", "failed", "error", "cancelled", "canceled", "timeout", "running", "pending", "queued", "fallback", "skipped", "dry_run", "used", "not_needed", "disabled", "missing_dependencies", "building", "deployed"}
ERRORS = {"error", "failed", "timeout", "cancelled", "canceled", "missing_dependencies"}


def discover_sources(home):
    """Read section names and enabled booleans only, without exporting config values."""
    sources = {}
    try:
        path = home/"config.toml"
        if path.stat().st_size > 1024*1024:
            return sources
        content = path.read_text(encoding="utf-8-sig")
        if tomllib:
            try:
                config = tomllib.loads(content)
            except ValueError:
                return sources
            servers = config.get("mcp_servers", {})
            if isinstance(servers, dict):
                sources.update({name: "configured" for name, value in servers.items() if SOURCE.fullmatch(name) and isinstance(value, dict) and value.get("enabled", True) is True})
            plugins = config.get("plugins", {})
            if isinstance(plugins, dict):
                sources.update({PLUGIN_SOURCES[name.split("@", 1)[0]]: "configured" for name, value in plugins.items() if name.split("@", 1)[0] in PLUGIN_SOURCES and isinstance(value, dict) and value.get("enabled", True) is True})
            return sources
        section, enabled = None, True
        multiline = None

        def finish():
            if section and enabled:
                sources[section] = "configured"

        for line in content.splitlines():
            if multiline:
                if line.count(multiline) % 2:
                    multiline = None
                continue
            if any(line.count(quote) % 2 for quote in ('"""', "'''")):
                multiline = '"""' if line.count('"""') % 2 else "'''"
                continue
            if line.lstrip().startswith("["):
                finish()
                section, enabled = None, True
                match = re.fullmatch(r'\s*\[mcp_servers\.(?:"([A-Za-z0-9_.-]+)"|([A-Za-z0-9_-]+))\]\s*(?:#.*)?', line)
                if match:
                    candidate = match[1] or match[2]
                    section = candidate if SOURCE.fullmatch(candidate) else None
                plugin = re.fullmatch(r'\s*\[plugins\."([^"@]+)@[^"\n]+"\]\s*(?:#.*)?', line)
                if plugin:
                    section = PLUGIN_SOURCES.get(plugin[1])
            elif section:
                match = re.fullmatch(r"\s*enabled\s*=\s*(true|false)\s*(?:#.*)?", line)
                if match:
                    enabled = match[1] == "true"
        finish()
    except (OSError, UnicodeError):
        pass
    return sources


def identity(tool):
    if tool in ("web__run", "web.run", "web_run", "functions.web__run"):
        return "web", "run"
    match = re.fullmatch(r"mcp__([A-Za-z0-9_.-]{1,80})__(.+)", tool)
    return (match[1], match[2][:160]) if match else (None, None)


def category(server, tool="", overrides=None):
    if overrides and server in overrides:
        return overrides[server]
    if server in PRESETS:
        return PRESETS[server]
    label = (server+" "+tool).lower()
    for words, group in ((('vercel', 'sites_', 'deploy', 'runtime_logs'), 'deployment'), (('security', 'review', 'pull_requests_checks'), 'review'), (('figma', 'notion', 'pages', 'pets', 'artifact'), 'apps'), (('browser', 'chrome', 'playwright'), 'runtime')):
        if any(word in label for word in words):
            return group
    return "other"


def action(tool):
    parts = set(re.split(r"[^a-z]+", tool.lower()))
    for words, label in (({"deploy", "deployment", "publish"}, "deployment"), ({"delete", "remove", "archive", "cancel", "reset"}, "change"), ({"create", "update", "write", "edit", "send", "attach", "move", "restore", "handoff", "fork"}, "write"), ({"get", "read", "list", "search", "find", "status", "inspect", "compare", "extract", "checks"}, "read")):
        if parts & words:
            return label
    return "operation"


def number(value):
    return value if type(value) in (int, float) and 0 <= value <= 2**53 else None


def resource_ids(args):
    ids = {}
    for key in ("page_id", "pageId", "threadId", "runId", "operationId", "deploymentId", "projectId", "file_key"):
        value = args.get(key)
        if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_.:-]{1,160}", value):
            ids[key] = value
    return ids


def request_metadata(server, tool, args):
    args = args if isinstance(args, dict) else {}
    result = {"resources": resource_ids(args)}
    if server == "local_documents":
        source = args.get("source") or args.get("target")
        if isinstance(source, str):
            extension = source.rsplit(".", 1)[-1].lower()
            if extension in {"pdf", "docx", "pptx", "xlsx", "csv", "txt", "md", "png", "jpg", "jpeg", "webp"}:
                result["format"] = extension
        if args.get("ocr") in ("auto", "force", "off"):
            result["ocr_mode"] = args["ocr"]
        for key in ("write", "write_output"):
            if type(args.get(key)) is bool:
                result[key] = args[key]
    if server == "workspace_inspection" and type(args.get("max_files")) is int:
        result["max_files"] = number(args["max_files"])
    if server == "web":
        result["references"] = list(dict.fromkeys(url for command in ("open", "click", "find", "screenshot") for item in (args[command][:30] if isinstance(args.get(command), list) else []) if isinstance(item, dict) for url in [reference_url(item.get("ref_id"))] if url))[:30]
        result["operations"] = [key for key in ("search_query", "open", "click", "find", "screenshot", "image_query", "finance", "weather", "sports", "time") if isinstance(args.get(key), list) and args[key]]
    return result


def mcp_operations(calls, include_web=True):
    events = []
    for tool, args, nested in calls:
        server, operation = identity(tool)
        if server and (server != "web" or include_web):
            events.append({"server": server, "tool": operation, "nested": nested, "category": category(server, operation), "action": action(operation), "metadata": request_metadata(server, operation, args)})
    return events[:100]


def reference_url(value):
    if not isinstance(value, str) or len(value) > 2048:
        return None
    try:
        parsed = urlsplit(value)
        host = parsed.hostname or ""
        if parsed.scheme not in ("http", "https") or parsed.username or parsed.password or "." not in host or host.endswith((".local", ".internal")):
            return None
        try:
            if not ipaddress.ip_address(host).is_global:
                return None
        except ValueError:
            pass
        # Reference links retain the page path, without query strings or fragments.
        return urlunsplit((parsed.scheme, parsed.netloc, parsed.path, "", ""))
    except ValueError:
        return None


def decoded_output(output):
    """Unwrap a single MCP structured result; reject unstructured tool logs."""
    if isinstance(output, str):
        if len(output) > 1024*1024:
            return {}
        try:
            output = json.loads(output)
        except (ValueError, RecursionError):
            return {}
    if not isinstance(output, dict):
        return {}
    result = output.get("structuredContent")
    if isinstance(result, dict):
        return dict(result, isError=output.get("isError") is True)
    content = output.get("content")
    if isinstance(content, list) and len(content) == 1 and isinstance(content[0], dict):
        text = content[0].get("text")
        if isinstance(text, str):
            try:
                result = json.loads(text)
                if isinstance(result, dict):
                    return dict(result, isError=output.get("isError") is True)
            except (ValueError, RecursionError):
                pass
    return output


def response_metadata(server, output):
    value = decoded_output(output)
    result = {}
    if server == "web":
        raw = output if isinstance(output, str) else json.dumps(output, ensure_ascii=False)
        result["references"] = list(dict.fromkeys(url for match in re.findall(r'https?://[^\s<>"\]\)]+', raw[:1024*1024]) for url in [reference_url(match.rstrip(".,;\\"))] if url))[:30]
    status = value.get("status", value.get("state"))
    status = status.lower() if isinstance(status, str) else None
    if value.get("isError") is True or value.get("error"):
        status = "error"
    if status in STATUSES or isinstance(status, str) and re.fullmatch(r"[a-z][a-z_]{0,39}", status):
        result["status"] = status
    result.update(resource_ids(value))
    for key in ("latency_ms", "duration_ms", "http_attempts", "retries", "input_tokens", "output_tokens"):
        item = number(value.get(key))
        if item is not None:
            result[key] = item
    if server == "local_documents":
        for key in ("written", "truncated"):
            if type(value.get(key)) is bool:
                result[key] = value[key]
        if number(value.get("content_chars")) is not None:
            result["content_chars"] = value["content_chars"]
        ocr = value.get("ocr")
        if isinstance(ocr, dict):
            if ocr.get("status") in STATUSES:
                result["ocr_status"] = ocr["status"]
            for key in ("eligible_items", "processed_items", "omitted_items"):
                if number(ocr.get(key)) is not None:
                    result["ocr_"+key] = ocr[key]
            if isinstance(ocr.get("errors"), list):
                result["ocr_errors"] = len(ocr["errors"])
    if server == "workspace_inspection":
        for key in ("changed", "only_in_source", "only_in_target", "missing", "added", "removed"):
            if isinstance(value.get(key), list):
                result[key+"_files"] = len(value[key])
        runs = value.get("runs")
        if isinstance(runs, list):
            result["evidence_runs"] = len(runs)
            counts = Counter(item.get("status") for run in runs[:100] if isinstance(run, dict) and isinstance(run.get("results"), list) for item in run["results"][:1000] if isinstance(item, dict) and isinstance(item.get("status"), str))
            for status in ("passed", "failed", "skipped"):
                result["checks_"+status] = counts[status]
    findings = value.get("findings")
    if isinstance(findings, list):
        result["findings"] = len(findings)
        counts = Counter(item.get("severity", "").lower() for item in findings[:1000] if isinstance(item, dict) and isinstance(item.get("severity", ""), str))
        for severity in ("critical", "high", "medium", "low"):
            if counts[severity]:
                result["findings_"+severity] = counts[severity]
    return result


def summarize(sources, events, enabled=None, overrides=None):
    enabled, overrides = enabled or {}, overrides or {}
    catalog = dict(sources)
    for event in events:
        catalog.setdefault(event["server"], "observed")
    visible = []
    for event in events:
        if enabled.get(event["server"], True):
            visible.append(event | {"category": category(event["server"], event["tool"], overrides)})
    servers = []
    for server, origin in sorted(catalog.items()):
        rows = [item for item in visible if item["server"] == server]
        direct = [item for item in rows if not item["nested"]]
        known = [item for item in direct if item["result"].get("status")]
        errors = sum(item["result"].get("status") in ERRORS for item in known)
        durations = sorted(item["duration_ms"] for item in direct if item.get("duration_ms") is not None)
        servers.append({"server": server, "origin": origin, "category": category(server, overrides=overrides), "enabled": enabled.get(server, True), "calls": len(direct), "recognized": len(rows)-len(direct), "returned": sum(bool(item.get("completed_at")) for item in direct), "known_status": len(known), "errors": errors, "average_ms": round(sum(durations)/len(durations)) if durations else None, "p95_ms": durations[min(len(durations)-1, int(len(durations)*.95))] if durations else None, "last_at": max((item["timestamp"] or "" for item in rows), default=None)})
    visible.sort(key=lambda item: item["timestamp"] or "", reverse=True)
    return {"servers": servers, "events": visible[:1000], "total_events": len(visible), "categories": CATEGORIES}
