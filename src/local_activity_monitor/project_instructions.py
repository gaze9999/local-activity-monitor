"""Read only explicitly selected root instruction files on demand."""
from pathlib import Path
from datetime import datetime, timezone
import os
import re

READ_LIMIT = 64*1024


def instructions(roots):
    files = []
    for root in roots[:32]:
        path = Path(root)/"AGENTS.md"
        item = {"file": str(path), "filename": "AGENTS.md", "root": str(root), "text": None, "truncated": False, "read_bytes": 0, "modified_at": None, "health": "missing"}
        try:
            if str(root).startswith(("\\\\", "//")) or not Path(root).is_absolute() or any(part.is_symlink() for part in (path, *path.parents)):
                item["health"] = "rejected"
            elif path.is_file():
                with path.open("rb") as stream:
                    stat = os.fstat(stream.fileno())
                    raw = stream.read(READ_LIMIT)
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
