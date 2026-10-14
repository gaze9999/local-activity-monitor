"""Sample this monitor process and its explicitly owned storage files."""
from datetime import datetime, timezone
from pathlib import Path
import os
import time

from .history_store import unsafe
from .performance_debug import process_memory


class MonitorUsage:
    def __init__(self):
        self.paths = ()
        self.previous = (time.monotonic(), time.process_time())
        self.checked = 0
        self.value = {}

    def sample(self):
        now, cpu = time.monotonic(), time.process_time()
        if self.value and now-self.checked < 5:
            return dict(self.value)
        elapsed, used = now-self.previous[0], cpu-self.previous[1]
        disk, files, health = 0, 0, 'ok'
        for path in dict.fromkeys(self.paths):
            path = Path(path)
            try:
                if unsafe(path.parent) or unsafe(path):
                    health = 'partly_unavailable'
                elif path.is_file():
                    disk += path.stat().st_size
                    files += 1
            except OSError:
                health = 'partly_unavailable'
        self.previous, self.checked = (now, cpu), now
        self.value = {**process_memory(), 'pid': os.getpid(),
                      'cpu_percent': round(used/elapsed*100, 1) if elapsed > 0 else None,
                      'disk_bytes': disk if health=='ok' else None,
                      'storage_files': files if health=='ok' else None, 'storage_health': health,
                      'checked_at': datetime.now(timezone.utc).isoformat(timespec='seconds')}
        return dict(self.value)
