"""Read bounded plugin configuration and cache manifests without runtime arguments."""
import json
import re
from itertools import islice

from .codex_metadata import text

PLUGIN_LIMIT = 256
MANIFEST_LIMIT = 65536
READ_LIMIT = 2*1024*1024
NAME = re.compile(r"[A-Za-z0-9_.-]{1,160}\Z")


def read_plugins(home, source_info=None):
    root, config = home/"plugins/cache", home/"config.toml"
    report = {"name": "plugins", "location": str(root), "health": "missing", "fields": ["name", "version", "provider", "enabled", "skills_count", "mcp_count"], "row_limit": PLUGIN_LIMIT, "manifest_bytes": MANIFEST_LIMIT, "byte_limit": READ_LIMIT, "bytes_read": 0, "rows_read": 0}
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
    try:
        candidates = []
        for provider in sorted(islice(root.iterdir(), PLUGIN_LIMIT)):
            if provider.is_symlink() or not provider.is_dir() or not NAME.fullmatch(provider.name):
                continue
            for plugin in sorted(islice(provider.iterdir(), PLUGIN_LIMIT)):
                if plugin.is_symlink() or not plugin.is_dir() or not NAME.fullmatch(plugin.name):
                    continue
                if len(candidates) >= PLUGIN_LIMIT:
                    break
                versions = [path for path in islice(plugin.iterdir(), 64) if not path.is_symlink() and path.is_dir() and re.fullmatch(r"\d+(?:\.\d+)*(?:[-+][A-Za-z0-9_.-]+)?", path.name)]
                if versions:
                    version = max(versions, key=lambda path: tuple(int(part) for part in re.match(r"\d+(?:\.\d+)*", path.name)[0].split(".")))
                    candidates.append((provider.name, plugin.name, version))
        report["health"] = "ok"
        for provider, name, directory in candidates:
            if report["bytes_read"] >= READ_LIMIT:
                report["health"] = "partial"
                break
            path = directory/".codex-plugin/plugin.json"
            if path.is_symlink() or path.parent.is_symlink():
                continue
            try:
                with path.open("rb") as stream:
                    raw = stream.read(min(MANIFEST_LIMIT+1, READ_LIMIT-report["bytes_read"]))
                report["bytes_read"] += len(raw)
                if len(raw) > MANIFEST_LIMIT:
                    continue
                value = json.loads(raw)
                if not isinstance(value, dict):
                    continue
                interface = value.get("interface")
                item = {"id": name+"@"+provider, "name": name, "provider": provider, "version": text(value.get("version"), 80) or directory.name, "enabled": configured.get((provider, name)), "config_present": (provider, name) in configured, "state": "configured" if (provider, name) in configured else "cached", "display_name": text(interface.get("displayName"), 160) if isinstance(interface, dict) else None, "description": text(value.get("description"), 512), "skills_count": None, "mcp_count": None}
                skills = directory/"skills"
                if value.get("skills") in ("./skills/", "./skills", "skills", "skills/") and not skills.is_symlink():
                    item["skills_count"] = sum(1 for skill in islice(skills.iterdir(), PLUGIN_LIMIT) if skill.is_dir() and not skill.is_symlink() and (skill/"SKILL.md").is_file()) if skills.is_dir() else 0
                elif "skills" not in value:
                    item["skills_count"] = 0
                servers = value.get("mcpServers")
                if isinstance(servers, dict):
                    item["mcp_count"] = len(servers)
                elif servers is None:
                    item["mcp_count"] = 0
                elif isinstance(servers, str) and servers in ("./.mcp.json", ".mcp.json"):
                    mcp = directory/".mcp.json"
                    if not mcp.is_symlink() and mcp.is_file() and report["bytes_read"]+mcp.stat().st_size <= READ_LIMIT and mcp.stat().st_size <= MANIFEST_LIMIT:
                        raw = mcp.read_bytes()
                        report["bytes_read"] += len(raw)
                        definition = json.loads(raw)
                        if isinstance(definition, dict) and isinstance(definition.get("mcpServers"), dict):
                            item["mcp_count"] = len(definition["mcpServers"])
                items[(provider, name)] = item
                report["rows_read"] += 1
            except (OSError, ValueError, RecursionError):
                report["health"] = "partly_unavailable"
    except OSError:
        report["health"] = "unavailable" if root.exists() else "missing"
    for (provider, name), enabled in configured.items():
        items.setdefault((provider, name), {"id": name+"@"+provider, "name": name, "provider": provider, "version": None, "enabled": enabled, "config_present": True, "state": "configured", "skills_count": None, "mcp_count": None})
    if source_info is not None:
        source_info[str(root)+"#plugins"] = report
    return {"items": list(items.values())[:PLUGIN_LIMIT], "health": report["health"], "row_limit": PLUGIN_LIMIT}
