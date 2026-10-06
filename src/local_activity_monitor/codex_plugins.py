"""Read bounded plugin configuration and cache metadata without runtime arguments."""
import json
import re
from itertools import islice

from .codex_metadata import text

PLUGIN_LIMIT = 256
MANIFEST_LIMIT = 65536
READ_LIMIT = 2*1024*1024
DIRECTORY_LIMIT = 4096
NAME = re.compile(r"[A-Za-z0-9_.-]{1,160}\Z")
VERSION = re.compile(r"\d+(?:\.\d+)*(?:[-+][A-Za-z0-9_.-]+)?\Z")


def _json_metadata(directory, relative, report):
    path = directory/relative
    try:
        if directory.resolve() != directory.absolute() or path.resolve() != path.absolute():
            return None, "unavailable"
        remaining = READ_LIMIT-report["bytes_read"]
        if remaining <= 0:
            return None, "truncated"
        with path.open("rb") as stream:
            raw = stream.read(min(MANIFEST_LIMIT+1, remaining))
        report["bytes_read"] += len(raw)
        if len(raw) > MANIFEST_LIMIT or len(raw) == remaining:
            return None, "truncated"
        value = json.loads(raw)
        return (value, "available") if isinstance(value, dict) else (None, "unavailable")
    except FileNotFoundError:
        return None, "missing"
    except (OSError, ValueError, RuntimeError):
        return None, "unavailable"


def _directories(path, report, limit):
    """Inspect only bounded direct entries, rejecting links at each level."""
    try:
        if path.resolve() != path.absolute():
            return [], "unavailable"
        remaining = DIRECTORY_LIMIT-report["directories_read"]
        entries = list(islice(path.iterdir(), min(limit+1, max(remaining, 0))))
        report["directories_read"] += len(entries)
        status = "truncated" if len(entries) > limit or remaining <= len(entries) else "available"
        return [entry for entry in entries[:limit] if NAME.fullmatch(entry.name) and entry.resolve() == entry.absolute() and entry.is_dir()], status
    except FileNotFoundError:
        return [], "missing"
    except (OSError, RuntimeError):
        return [], "unavailable"


def _plugin_metadata(directory, report):
    value, status = _json_metadata(directory, ".codex-plugin/plugin.json", report)
    source = ".codex-plugin/plugin.json"
    if status == "missing":
        value, status = _json_metadata(directory, "plugin.json", report)
        source = "plugin.json"
    item = {"version": directory.name if VERSION.fullmatch(directory.name) else None, "metadata_status": status, "manifest_source": source if value is not None else None, "skills_count": None, "mcp_count": None, "skills": None, "mcp_servers": None, "components": [], "skills_status": "not_read", "mcp_status": "not_read"}
    if value is None:
        return item
    item["version"] = text(value.get("version"), 80) or item["version"]
    interface = value.get("interface")
    extensions = value.get("extensions")
    if not isinstance(interface, dict) and isinstance(extensions, dict):
        extension = extensions.get("com.openai")
        interface = extension.get("interface") if isinstance(extension, dict) else None
    item["display_name"] = text(interface.get("displayName"), 160) if isinstance(interface, dict) else None
    item["description"] = text(value.get("description"), 512)
    item["components"] = [key for key in ("skills", "mcpServers", "apps", "hooks", "commands", "agents") if key in value]
    if "skills" not in value or value["skills"] in ("./skills/", "./skills", "skills", "skills/"):
        entries, state = _directories(directory/"skills", report, PLUGIN_LIMIT)
        try:
            names = sorted(entry.name for entry in entries if (entry/"SKILL.md").resolve() == (entry/"SKILL.md").absolute() and (entry/"SKILL.md").is_file())
        except (OSError, RuntimeError):
            names, state = [], "unavailable"
        item["skills_status"] = state
        if state in ("available", "missing", "truncated"):
            missing_declared = state == "missing" and "skills" in value
            item["skills"] = None if missing_declared else names
            item["skills_count"] = len(names) if state != "truncated" and not missing_declared else None
            if names and "skills" not in item["components"]:
                item["components"].append("skills")
    else:
        item["skills_status"] = "unsupported"
    servers = value.get("mcpServers")
    if isinstance(servers, dict):
        state = "available"
    elif "mcpServers" not in value:
        servers, state = {}, "missing"
    elif servers in ("./.mcp.json", ".mcp.json"):
        definition, state = _json_metadata(directory, ".mcp.json", report)
        servers = definition.get("mcpServers") if definition else None
        if state == "available" and not isinstance(servers, dict):
            state = "unavailable"
    else:
        state = "unsupported"
    if isinstance(servers, dict):
        names = sorted(key for key in servers if isinstance(key, str) and NAME.fullmatch(key))
        if len(names) > PLUGIN_LIMIT:
            state = "truncated"
        item["mcp_servers"] = names[:PLUGIN_LIMIT]
        item["mcp_count"] = len(names) if state != "truncated" else None
        if len(names) != len(servers) and state != "truncated":
            state = "unavailable"
            item["mcp_count"] = None
    item["mcp_status"] = state
    return item


def read_plugins(home, source_info=None):
    root, config = home/"plugins/cache", home/"config.toml"
    report = {"name": "plugins", "location": str(root), "health": "missing", "fields": ["name", "version", "provider", "enabled", "skills_count", "mcp_count", "skills", "mcp_servers", "components", "metadata_status", "manifest_source", "skills_status", "mcp_status"], "row_limit": PLUGIN_LIMIT, "manifest_bytes": MANIFEST_LIMIT, "byte_limit": READ_LIMIT, "directory_limit": DIRECTORY_LIMIT, "directories_read": 0, "bytes_read": 0, "rows_read": 0}
    configured, items = {}, {}
    try:
        if not config.is_symlink():
            with config.open("rb") as stream:
                raw = stream.read(MANIFEST_LIMIT+1)
            report["bytes_read"] += len(raw)
            if len(raw) <= MANIFEST_LIMIT:
                current = None
                for line in raw.decode("utf-8").splitlines():
                    if line.lstrip().startswith("["):
                        match = re.fullmatch(r'\s*\[plugins\."([A-Za-z0-9_.-]+)@([A-Za-z0-9_.-]+)"\]\s*(?:#.*)?', line)
                        current = (match[2], match[1]) if match else None
                        if current and len(configured) < PLUGIN_LIMIT:
                            configured.setdefault(current, None)
                    if current in configured:
                        match = re.fullmatch(r"\s*enabled\s*=\s*(true|false)\s*(?:#.*)?", line)
                        if match:
                            configured[current] = match[1] == "true"
    except (OSError, UnicodeError):
        pass
    providers, root_status = _directories(root, report, PLUGIN_LIMIT)
    report["health"] = "ok" if root_status == "available" else "partial" if root_status == "truncated" else root_status
    for provider in sorted(providers):
        plugins, status = _directories(provider, report, PLUGIN_LIMIT)
        if status != "available":
            report["health"] = "partial" if status == "truncated" else "partly_unavailable"
        for plugin in sorted(plugins):
            if len(items) >= PLUGIN_LIMIT or report["bytes_read"] >= READ_LIMIT:
                report["health"] = "partial"
                break
            versions, status = _directories(plugin, report, 64)
            if status != "available":
                report["health"] = "partial" if status == "truncated" else "partly_unavailable"
            numeric = [path for path in versions if VERSION.fullmatch(path.name)]
            latest = next((path for path in versions if path.name == "latest"), None)
            directory = max(numeric, key=lambda path: tuple(int(part) for part in re.match(r"\d+(?:\.\d+)*", path.name)[0].split("."))) if numeric else latest
            if directory is None:
                continue
            key = (provider.name, plugin.name)
            item = {"id": plugin.name+"@"+provider.name, "name": plugin.name, "provider": provider.name, "enabled": configured.get(key), "config_present": key in configured, "state": "configured" if key in configured else "cached", **_plugin_metadata(directory, report)}
            missing_component = any(item[field+"_status"] == "missing" and item[field+"_count"] is None for field in ("skills", "mcp"))
            if item["metadata_status"] == "missing" or missing_component or any(item[field] in ("unavailable", "unsupported") for field in ("metadata_status", "skills_status", "mcp_status")):
                report["health"] = "partly_unavailable"
            elif any(item[field] == "truncated" for field in ("metadata_status", "skills_status", "mcp_status")):
                report["health"] = "partial"
            items[key] = item
            report["rows_read"] += 1
    for (provider, name), enabled in configured.items():
        items.setdefault((provider, name), {"id": name+"@"+provider, "name": name, "provider": provider, "version": None, "enabled": enabled, "config_present": True, "state": "configured", "metadata_status": "missing", "manifest_source": None, "skills_count": None, "mcp_count": None, "skills": None, "mcp_servers": None, "components": [], "skills_status": "not_read", "mcp_status": "not_read"})
    if source_info is not None:
        source_info[str(root)+"#plugins"] = report
    return {"items": list(items.values())[:PLUGIN_LIMIT], "health": report["health"], "row_limit": PLUGIN_LIMIT}
