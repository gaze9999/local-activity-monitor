"""Manual, bounded Codex Cloud metadata queries through the signed-in CLI."""

import copy
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import threading
import time
from datetime import datetime, timezone

from .codex_account import _executable
from .codex_metadata import date, text


def _timestamp(value):
    if isinstance(value, str) and len(value) <= 64 and "T" in value:
        try:
            parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
            if parsed.tzinfo is not None:
                return parsed.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        except ValueError:
            pass
        return None
    return date(value)


class CodexCloudSource:
    CACHE_SECONDS = 60
    MAX_BYTES = 1024 * 1024
    TIMEOUT = 10
    PAGE_LIMIT = 20

    def __init__(self, home):
        self.home = Path(home)
        self.cache = {}
        self.lock = threading.Lock()

    def query(self, enabled=False, cursor=None):
        if cursor is not None and (not isinstance(cursor, str) or not re.fullmatch(r"[A-Za-z0-9_+/=:.\-]{1,2048}", cursor)):
            raise ValueError("Invalid cloud cursor")
        if enabled is not True:
            return {"health": "disabled", "items": [], "cursor": None}
        with self.lock:
            cached = self.cache.get(cursor)
            if cached and time.monotonic()-cached[0] < self.CACHE_SECONDS:
                return copy.deepcopy(cached[1])
            result = {"health": "unavailable", "items": [], "cursor": None,
                      "checked_at": datetime.now(timezone.utc).isoformat()}
            if executable := _executable():
                try:
                    result.update(self._read(executable, cursor), health="ok")
                except (OSError, ValueError, RecursionError, queue.Empty):
                    result["health"] = "error"
            # At most ten independently cached pages, including failures.
            if len(self.cache) >= 10 and cursor not in self.cache:
                self.cache.pop(next(iter(self.cache)))
            self.cache[cursor] = (time.monotonic(), result)
            return copy.deepcopy(result)

    def _read(self, executable, cursor):
        command = [executable, "cloud", "list", "--json", "--limit", str(self.PAGE_LIMIT), "-c", "analytics.enabled=false"]
        if cursor is not None:
            command.extend(("--cursor", cursor))
        env = dict(os.environ)
        env["CODEX_HOME"] = str(self.home)
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL, env=env, shell=False,
                                   creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        inbox = queue.Queue(maxsize=1)

        def read_output():
            try:
                inbox.put(process.stdout.read(self.MAX_BYTES+1))
            except OSError:
                inbox.put(None)

        reader = threading.Thread(target=read_output, name="codex-cloud-reader", daemon=True)
        reader.start()
        deadline = time.monotonic()+self.TIMEOUT
        try:
            raw = inbox.get(timeout=self.TIMEOUT)
            if raw is None or len(raw) > self.MAX_BYTES:
                raise ValueError("Cloud output limit")
            try:
                code = process.wait(timeout=max(.001, deadline-time.monotonic()))
            except subprocess.TimeoutExpired:
                raise ValueError("Cloud query timed out") from None
            if code != 0:
                raise ValueError("Cloud query failed")
            return self._project(json.loads(raw))
        finally:
            # End only this query's child, including timeouts and oversized output.
            if process.poll() is None:
                try:
                    process.terminate()
                    process.wait(timeout=.5)
                except (OSError, subprocess.TimeoutExpired):
                    try:
                        process.kill()
                        process.wait(timeout=.5)
                    except (OSError, subprocess.TimeoutExpired):
                        pass
            reader.join(timeout=.5)
            process.stdout.close()

    @classmethod
    def _project(cls, value):
        rows = value.get("tasks") if isinstance(value, dict) else None
        if not isinstance(rows, list) or len(rows) > cls.PAGE_LIMIT:
            raise ValueError("Invalid cloud task list")
        items = []
        for row in rows:
            identity = row.get("id") if isinstance(row, dict) else None
            if not isinstance(identity, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,160}", identity):
                raise ValueError("Invalid cloud task identity")
            item = {"id": identity, "title": text(row.get("title")), "updated_at": _timestamp(row.get("updated_at")),
                    "environment_label": text(row.get("environment_label"), 160)}
            status = row.get("status")
            item["status"] = status if isinstance(status, str) and re.fullmatch(r"[A-Za-z_]{1,40}", status) else None
            if isinstance(row.get("is_review"), bool):
                item["is_review"] = row["is_review"]
            attempts = row.get("attempt_total")
            if type(attempts) is int and 0 <= attempts <= 1000000:
                item["attempt_total"] = attempts
            items.append(item)
        cursor = value.get("cursor")
        if cursor is not None and (not isinstance(cursor, str) or not re.fullmatch(r"[A-Za-z0-9_+/=:.\-]{1,2048}", cursor)):
            raise ValueError("Invalid cloud next cursor")
        return {"items": items, "cursor": cursor}
