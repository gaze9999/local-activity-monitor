"""Detect local Codex changes using bounded file metadata, without reading bodies."""
from datetime import datetime, timedelta, timezone
import os
import re
import time

from .collectors import now


class IdleActivity:
    FILE_LIMIT = 5000
    ENTRY_LIMIT = 10000

    def __init__(self, home):
        self.home = home
        self.signatures = None
        self.last_change = time.monotonic()
        self.last_activity_at = None
        self.paused = False
        self.health = "waiting"

    def wake(self):
        self.last_change = time.monotonic()
        self.paused = False

    def check(self, collector, minutes, metadata=True):
        if not collector or not minutes:
            self.signatures = None
            self.wake()
            self.health = "disabled"
            return
        paths = set(list(collector.files)[:self.FILE_LIMIT])
        dates = {datetime.now().date(), datetime.now(timezone.utc).date()}
        dates |= {day-timedelta(days=1) for day in dates}
        root = self.home/"sessions"
        paths.add(root)
        try:
            for day in dates:
                year = root/str(day.year)
                month = year/f"{day.month:02}"
                directory = month/f"{day.day:02}"
                paths.update((year, month, directory))
                if not directory.exists():
                    continue
                with os.scandir(directory) as entries:
                    for index, entry in enumerate(entries):
                        if index >= self.ENTRY_LIMIT or len(paths) > self.FILE_LIMIT+32:
                            raise OSError("Activity metadata limit")
                        if entry.name.startswith("rollout-") and entry.name.endswith(".jsonl") and entry.is_file(follow_symlinks=False):
                            paths.add(directory/entry.name)
            if metadata:
                paths.update((self.home/".codex-global-state.json", self.home/"sqlite/codex-dev.db", self.home/"sqlite/codex-dev.db-wal"))
                if self.home.exists():
                    with os.scandir(self.home) as entries:
                        for index, entry in enumerate(entries):
                            if index >= self.ENTRY_LIMIT:
                                raise OSError("Activity metadata limit")
                            if re.fullmatch(r"state_\d+\.sqlite(?:-wal)?", entry.name):
                                paths.add(self.home/entry.name)
                                if len(paths) > self.FILE_LIMIT+64:
                                    raise OSError("Activity metadata limit")
            signatures = {}
            for path in paths:
                if path.is_symlink():
                    raise OSError("Activity symlink")
                try:
                    stat = path.stat()
                    signatures[str(path)] = (stat.st_mtime_ns, stat.st_size)
                except FileNotFoundError:
                    signatures[str(path)] = None
            if self.signatures is not None and signatures != self.signatures:
                self.wake()
                self.last_activity_at = now()
                collector.next_scan = 0
            self.signatures = signatures
            self.health = "ok"
            pending = any(state.get("offset") is None or state.get("history_cursor") or state.get("error_cursor") for state in collector.files.values())
            self.paused = not pending and time.monotonic()-self.last_change >= minutes*60
        except OSError:
            self.health = "unavailable"
            self.wake()

    def snapshot(self, minutes, interval):
        return {"paused": self.paused, "idle_minutes": minutes, "health": self.health,
                "last_activity_at": self.last_activity_at,
                "check_interval": min(30, max(10, interval)) if self.paused else interval}
