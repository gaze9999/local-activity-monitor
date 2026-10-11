"""Bounded, opt-in public OpenAI and Codex queries."""

import copy
import json
import re
import threading
import time
from datetime import datetime, timezone
from urllib.request import HTTPRedirectHandler, Request, build_opener


SOURCES = {
    "openai": ("OpenAI", "https://status.openai.com/api/v2/status.json"),
    "codex": ("Codex", "https://api.github.com/repos/openai/codex/releases/latest"),
}


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class PublicApiSource:
    CACHE_SECONDS = 60
    MAX_BYTES = 65536
    TIMEOUT = 4

    def __init__(self):
        self.cache = {}
        self.locks = {key: threading.Lock() for key in SOURCES}

    def query(self, source, enabled=False):
        if source not in SOURCES:
            raise ValueError("Unknown public source")
        name, url = SOURCES[source]
        if not enabled:
            return {"source": source, "name": name, "health": "disabled"}
        with self.locks[source]:
            cached = self.cache.get(source)
            if cached and time.monotonic()-cached[0] < self.CACHE_SECONDS:
                return copy.deepcopy(cached[1])
            result = {"source": source, "name": name, "url": url,
                      "checked_at": datetime.now(timezone.utc).isoformat()}
            try:
                result.update(self._read(url), health="ok")
            except (OSError, ValueError):
                result["health"] = "error"
            self.cache[source] = (time.monotonic(), result)
            return copy.deepcopy(result)

    def _read(self, url):
        limit = 1048576 if url == SOURCES["codex"][1] else self.MAX_BYTES
        request = Request(url, headers={"Accept": "application/json", "User-Agent": "LocalActivityMonitor"})
        with build_opener(NoRedirect()).open(request, timeout=self.TIMEOUT) as response:
            raw = response.read(limit+1)
        if len(raw) > limit:
            raise ValueError("Public status response too large")
        value = json.loads(raw)
        if url == SOURCES["codex"][1]:
            if not isinstance(value, dict) or value.get("draft") is not False or value.get("prerelease") is not False:
                raise ValueError("Invalid Codex release")
            tag, published = value.get("tag_name"), value.get("published_at")
            if not isinstance(tag, str) or not re.fullmatch(r"(?:rust-v|v)?\d+\.\d+\.\d+", tag) or not isinstance(published, str) or len(published)>64:
                raise ValueError("Invalid Codex release metadata")
            datetime.fromisoformat(published.replace("Z", "+00:00"))
            return {"version": re.sub(r"^(?:rust-v|v)", "", tag), "updated_at": published}
        status = value.get("status") if isinstance(value, dict) else None
        page = value.get("page") if isinstance(value, dict) else None
        if not isinstance(status, dict) or status.get("indicator") not in ("none", "minor", "major", "critical"):
            raise ValueError("Invalid public status")
        updated = page.get("updated_at") if isinstance(page, dict) else None
        if updated is not None:
            if not isinstance(updated, str) or len(updated) > 64:
                raise ValueError("Invalid public status time")
            datetime.fromisoformat(updated.replace("Z", "+00:00"))
        return {"indicator": status["indicator"], "updated_at": updated}
