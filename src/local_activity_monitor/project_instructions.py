"""Read only explicitly selected root instruction files on demand."""
from pathlib import Path
from datetime import datetime, timezone
import os
import re
import configparser
import json
import subprocess

READ_LIMIT = 64*1024
TOTAL_READ_LIMIT = 256*1024
DOCUMENT_LIMIT = 64


def local_file(path):
    return path.is_absolute() and not str(path).startswith(("\\\\", "//")) and not any(
        part.is_symlink() or getattr(part, "is_junction", lambda: False)()
        for part in (path, *path.parents))


def repository_id(value):
    return re.sub(r"\.git$", "", re.sub(r"^(?:https?://|ssh://git@|git@)", "", value).replace(":", "/")).rstrip("/")


def workspace_sources(root):
    """Resolve registered parent sources, never arbitrary links in Markdown."""
    parent = root.parent
    modules = parent/".gitmodules"
    if not local_file(modules) or not modules.is_file() or modules.stat().st_size > READ_LIMIT:
        return []
    config = configparser.ConfigParser(interpolation=None)
    config.read_string(modules.read_text(encoding="utf-8"))
    member = next((config[section] for section in config.sections()
                   if config[section].get("path") == root.name), None)
    if member is None:
        return []
    result = subprocess.run(["git", "-C", str(root), "config", "--get", "remote.origin.url"],
                            capture_output=True, text=True, timeout=2, check=False)
    if result.returncode or repository_id(result.stdout.strip()) != repository_id(member.get("url", "")):
        return []
    for registry_name, field, prefix in (("workspace.json", "instructions", "project-agents/"),
                                         ("governance/projects.json", "instructions", "governance/projects/")):
        registry = parent/registry_name
        if not local_file(registry) or not registry.is_file() or registry.stat().st_size > READ_LIMIT:
            continue
        value = json.loads(registry.read_text(encoding="utf-8"))
        projects = value.get("projects")
        if not isinstance(projects, list):
            return []
        project = next((item for item in projects if isinstance(item, dict) and item.get("path") == root.name), None)
        if project is None or not isinstance(project.get("repository"), str) or repository_id(project["repository"]) != repository_id(member.get("url", "")):
            return []
        source = project.get(field)
        if not isinstance(source, str) or not source.startswith(prefix) or not source.endswith("/AGENTS.md"):
            return []
        if "\\" in source or ":" in source or any(part in (".", "..", "") for part in source.split("/")):
            return []
        paths = [parent/"AGENTS.md", parent/source]
        if project.get("entry") == "AGENTS.override.md":
            paths.append(root/"AGENTS.md")
        return paths
    # A shared game entry is accepted only through the verified parent's fixed source.
    entry = parent/".codex/agent-guidance/game-entry.md"
    game = parent/".codex/agent-guidance/game-AGENTS.md"
    active = root/"AGENTS.md"
    if local_file(entry) and local_file(game) and local_file(active) and entry.is_file() and game.is_file() and active.is_file():
        if entry.stat().st_size <= READ_LIMIT and active.stat().st_size <= READ_LIMIT and active.read_bytes() == entry.read_bytes():
            return [parent/"AGENTS.md", game]
    return []


def instructions(roots):
    files = []
    seen = set()
    remaining = TOTAL_READ_LIMIT
    for root in roots[:32]:
        root = Path(root)
        paths = [root/"AGENTS.md"]
        try:
            override = root/"AGENTS.override.md"
            if local_file(override) and override.is_file() and override.stat().st_size:
                paths = [override]
            if local_file(root):
                paths = workspace_sources(root) + paths
        except (OSError, ValueError, configparser.Error, subprocess.SubprocessError):
            pass
        for path in paths:
            if str(path) in seen or len(files) >= DOCUMENT_LIMIT or remaining <= 0:
                continue
            seen.add(str(path))
            item = {"file": str(path), "filename": path.name, "root": str(root), "text": None, "truncated": False, "read_bytes": 0, "modified_at": None, "health": "missing"}
            try:
                if not local_file(path):
                    item["health"] = "rejected"
                elif path.is_file():
                    with path.open("rb") as stream:
                        stat = os.fstat(stream.fileno())
                        raw = stream.read(min(READ_LIMIT, remaining))
                    remaining -= len(raw)
                    value = raw.decode("utf-8", errors="replace")
                    value = re.sub(r"(?i)\bBearer\s+[A-Za-z0-9._~+/-]+=*|\bsk-[A-Za-z0-9_-]{12,}", "[已隱藏]", value)
                    value = re.sub(r'''(?i)\b(?:api[_-]?key|password|secret|authorization|access[_-]?token|token)["']?\s*[:=]\s*(?:"[^"]*"|'[^']*'|[^\s,;]+)''', "[已隱藏]", value)
                    item.update(text=value, truncated=stat.st_size > len(raw), read_bytes=len(raw), modified_at=datetime.fromtimestamp(stat.st_mtime, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"), health="ok")
            except OSError:
                item["health"] = "unavailable"
            files.append(item)
    documents = [item for item in files if item["health"] != "missing"]
    health = "missing" if not documents else "ok" if all(item["health"] == "ok" for item in documents) else "partly_unavailable"
    return {"documents": documents, "health": health, "read_limit": READ_LIMIT, "scope": "selected_root_instruction_files"}


def observed_file_metadata(event):
    """Only stat an already observed file. Never open or read its body."""
    result = {"modified_at": None, "bytes": None, "health": "missing"}
    path = Path(event["path"])
    if not path.is_absolute() and event.get("workdir"):
        path = Path(event["workdir"])/path
    try:
        if not path.is_absolute() or str(path).startswith(("\\\\", "//")) or any(part.is_symlink() for part in (path, *path.parents)) or path.name.lower() in ("auth.json", "credentials.json", ".env") or path.suffix.lower() in (".pem", ".key"):
            result["health"] = "rejected"
        elif path.is_file():
            stat = path.stat()
            result.update(modified_at=datetime.fromtimestamp(stat.st_mtime, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"), bytes=stat.st_size, health="ok")
    except (OSError, ValueError):
        result["health"] = "unavailable"
    return result
