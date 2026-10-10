"""Observe registered Git worktrees and the local Codex management directory."""
from datetime import datetime, timezone
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time


def local_path(value):
    if not isinstance(value, (str, Path)) or not str(value) or any(ord(char) < 32 or ord(char) == 127 for char in str(value)):
        return None
    path = Path(value)
    try:
        if not path.is_absolute() or str(path).startswith(("\\\\", "//")) or any(part.is_symlink() for part in (path, *path.parents)):
            return None
    except OSError:
        return None
    return path


def parse_worktrees(output):
    items, item = [], {}
    for field in output.decode("utf-8", "replace").split("\0"):
        if not field:
            if item.get("path"):
                items.append(item)
            item = {}
            continue
        key, _, value = field.partition(" ")
        if key == "worktree":
            item["path"] = value
        elif key == "HEAD" and re.fullmatch(r"(?:[a-fA-F0-9]{40}|[a-fA-F0-9]{64})", value):
            item["commit"] = value
        elif key == "branch" and value.startswith("refs/heads/") and len(value) <= 512 and not any(ord(char) < 32 for char in value):
            item["branch"] = value.removeprefix("refs/heads/")
        elif key in ("detached", "bare", "locked", "prunable"):
            item[key] = True
    if item.get("path"):
        items.append(item)
    return items


class WorktreeCollector:
    ROOT_LIMIT = 100
    ITEM_LIMIT = 500
    BYTE_LIMIT = 1024*1024
    CACHE_SECONDS = 30
    TIME_BUDGET = 5

    def __init__(self, home):
        self.managed_root = home/"worktrees"
        self.items = {}
        self.root_items = {}
        self.root_signature = None
        self.cursor = 0
        self.next_read = 0
        self.signature = None
        self.query_failure = None
        self.result = {"health": "waiting", "items": [], "checked_at": None}

    def git_list(self, root, timeout):
        self.query_failure = None
        git = shutil.which("git")
        if git is None:
            self.query_failure = {"reason": "missing_git"}
            return None, "unavailable"
        environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, GIT_OPTIONAL_LOCKS="0", GIT_TERMINAL_PROMPT="0")
        stage = "temporary_file_unavailable"
        try:
            with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
                stage = "launch_failed"
                process = subprocess.Popen([git, "--no-optional-locks", "-C", str(root), "worktree", "list", "--porcelain", "-z"],
                                           stdin=subprocess.DEVNULL, stdout=output, stderr=errors, env=environment,
                                           creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
                stage = "query_io_error"
                try:
                    process.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                    self.query_failure = {"reason": "timeout"}
                    return None, "timeout"
                if process.returncode:
                    errors.seek(0)
                    diagnostic = errors.read(4096).lower()
                    reason = "git_exit"
                    for message, code in ((b"dubious ownership", "ownership_rejected"), (b"not a git repository", "not_repository"),
                                          (b"unknown option", "unsupported_option"), (b"permission denied", "permission_denied"),
                                          (b"invalid gitfile", "invalid_git_metadata")):
                        if message in diagnostic:
                            reason = code
                            break
                    self.query_failure = {"reason": reason, "exit_code": process.returncode}
                    return None, "unavailable"
                output.seek(0)
                raw = output.read(self.BYTE_LIMIT+1)
                if len(raw) > self.BYTE_LIMIT:
                    self.query_failure = {"reason": "oversized"}
                    return None, "oversized"
                return parse_worktrees(raw), "ok"
        except OSError as error:
            self.query_failure = {"reason": stage, "error_code": error.errno}
            return None, "unavailable"

    def refresh(self, projects, threads, workdirs, enabled=True):
        if not enabled:
            self.items.clear()
            self.root_items.clear()
            self.root_signature = None
            self.cursor = 0
            self.signature = None
            self.result = {"health": "disabled", "items": [], "checked_at": None}
            return self.snapshot()
        roots = {}
        for project_id, project in projects.items():
            if len(roots) > self.ROOT_LIMIT:
                break
            for value in project.get("folders", [])[:32]:
                if len(roots) > self.ROOT_LIMIT:
                    break
                path = local_path(value)
                if path is not None:
                    roots.setdefault(str(path), set()).add(project_id)
        signature = tuple((key, tuple(sorted(value))) for key, value in sorted(roots.items()))
        current = time.monotonic()
        if signature != self.signature or current >= self.next_read:
            self.signature, self.next_read = signature, current+self.CACHE_SECONDS
            self.read(roots)
        candidates = sorted(((Path(item["_path"]), item) for item in self.items.values()), key=lambda pair: len(pair[0].parts), reverse=True)
        for item in self.items.values():
            item["thread_ids"] = []
        paths = {}
        for thread in threads:
            value = workdirs.get(thread["thread_id"])
            if not isinstance(value, str):
                continue
            if value not in paths:
                paths[value] = local_path(value)
            path = paths[value]
            if path is not None:
                item = next((item for root, item in candidates if path.is_relative_to(root)), None)
                if item is not None:
                    item["thread_ids"].append(thread["thread_id"])
        for item in self.items.values():
            item["thread_count"] = len(item["thread_ids"])
        return self.snapshot()

    def notification_roots(self):
        """Watch only known, local Git metadata, without reading workspace content."""
        roots = set()
        paths = [key for key, _ in self.signature or ()]+[item['_path'] for item in self.items.values()]
        def metadata_path(base, value):
            candidate = Path(value)
            return local_path(candidate if candidate.is_absolute() else Path(os.path.abspath(base/candidate)))
        def marker(path):
            if local_path(path) is None or not path.is_file():
                return None
            with path.open('rb') as stream:
                value = stream.read(4097)
            return value.decode('utf-8').strip() if len(value) <= 4096 else None
        for value in dict.fromkeys(paths):
            path = local_path(value)
            if path is None:
                continue
            try:
                git = local_path(path/'.git')
                if git is not None and git.is_file():
                    value = marker(git)
                    git = metadata_path(path, value[8:]) if value and value.startswith('gitdir: ') else None
                if git is not None and git.is_dir():
                    roots.add(git)
                    common = marker(git/'commondir')
                    if common:
                        target = metadata_path(git, common)
                        if target is not None and target.is_dir():
                            roots.add(target)
                elif (path/'HEAD').is_file() and (path/'objects').is_dir():
                    roots.add(path)
            except (OSError, UnicodeError, ValueError):
                continue
            if len(roots) >= self.ROOT_LIMIT:
                break
        return sorted(roots, key=str)[:self.ROOT_LIMIT]

    def read(self, roots):
        checked = datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        managed_health, managed_dirs, failures, limited = "missing", 0, 0, False
        query_failures = []
        def failure(root_key, project_ids, detail):
            query_failures.append({"root_id": hashlib.sha256(root_key.encode("utf-8")).hexdigest()[:24], "project_ids": sorted(project_ids), **detail})
        try:
            managed = local_path(self.managed_root)
            if managed is None:
                managed_health = "rejected"
            elif managed.is_dir():
                managed_health = "ok"
                scanned = 0
                for folder in managed.iterdir():
                    scanned += 1
                    if scanned > self.ROOT_LIMIT or managed_dirs >= self.ROOT_LIMIT:
                        limited = True
                        break
                    if not folder.is_dir() or local_path(folder) is None:
                        continue
                    managed_dirs += 1
                    for index, child in enumerate(folder.iterdir()):
                        if len(roots) >= self.ROOT_LIMIT or index >= 32:
                            limited = True
                            break
                        if local_path(child) is not None and child.is_dir() and (child/".git").exists():
                            roots.setdefault(str(child), set())
        except OSError:
            managed_health = "unavailable"
        repositories, queries = {}, 0
        limited |= len(roots) > self.ROOT_LIMIT
        selected = list(roots.items())[:self.ROOT_LIMIT]
        signature = tuple((os.path.normcase(os.path.normpath(value)), tuple(sorted(ids))) for value, ids in selected)
        if signature != self.root_signature:
            self.items.clear()
            self.root_items.clear()
            self.cursor = 0
            self.root_signature = signature
        start = self.cursor
        deadline = time.monotonic()+self.TIME_BUDGET
        git_available = shutil.which("git") is not None
        for offset in range(len(selected)):
            index = (start+offset) % len(selected)
            value, project_ids = selected[index]
            self.cursor = (index+1) % len(selected)
            root = local_path(value)
            if root is None:
                continue
            key = os.path.normcase(os.path.normpath(value))
            if key in repositories:
                for identity in repositories[key]:
                    self.items[identity]["project_ids"] = sorted(set(self.items[identity]["project_ids"]) | project_ids)
                continue
            try:
                marker = root/".git"
                if marker.is_symlink():
                    failures += 1
                    failure(key, project_ids, {"reason": "linked_git_metadata"})
                    continue
                if not marker.exists() and not ((root/"HEAD").is_file() and (root/"objects").is_dir()):
                    removed = self.root_items.pop(key, set())
                    retained = set().union(*self.root_items.values())
                    for identity in removed-retained:
                        self.items.pop(identity, None)
                    continue
                if not git_available:
                    break
                remaining = deadline-time.monotonic()
                if remaining <= 0:
                    self.cursor = index
                    limited = True
                    break
                self.query_failure = None
                records, health = self.git_list(root, remaining)
                queries += 1
                if health != "ok":
                    failures += 1
                    failure(key, project_ids, self.query_failure or {"reason": health})
                    continue
                previous = self.root_items.get(key, set())
                for alias in [alias for alias, ids in self.root_items.items() if ids and ids == previous]:
                    del self.root_items[alias]
                retained = set().union(*self.root_items.values())
                for identity in previous-retained:
                    self.items.pop(identity, None)
                aliases = {os.path.normcase(os.path.normpath(record["path"])) for record in records}
                project_ids = set(project_ids).union(*(ids for seed, ids in selected if os.path.normcase(os.path.normpath(seed)) in aliases))
                identities = []
                for record in records:
                    path = local_path(record["path"])
                    if path is None or record.get("bare"):
                        continue
                    identity = hashlib.sha256(os.path.normcase(str(path)).encode("utf-8")).hexdigest()[:24]
                    if identity not in self.items and len(self.items) >= self.ITEM_LIMIT:
                        limited = True
                        continue
                    managed = path.is_relative_to(self.managed_root) or (path.parent/".codex-worktree-name").is_file()
                    self.items[identity] = {"id": identity, "name": path.name, "branch": record.get("branch"), "commit": record.get("commit"),
                                            "detached": bool(record.get("detached")), "locked": bool(record.get("locked")), "prunable": bool(record.get("prunable")),
                                            "available": path.is_dir(), "managed": managed, "project_ids": sorted(project_ids), "checked_at": checked,
                                            "_path": str(path), "thread_ids": [], "thread_count": 0}
                    identities.append(identity)
                for record in records:
                    repositories[os.path.normcase(os.path.normpath(record["path"]))] = identities
                for seed, _ in selected:
                    alias = os.path.normcase(os.path.normpath(seed))
                    if alias == key or alias in aliases:
                        self.root_items[alias] = set(identities)
            except (OSError, ValueError) as error:
                failures += 1
                failure(key, project_ids, {"reason": "metadata_io_error", "error_code": error.errno if isinstance(error, OSError) else None})
        complete = git_available and not failures and not limited and managed_health not in ("unavailable", "rejected")
        self.result = {"health": "missing_git" if not git_available else "partly_unavailable" if failures or managed_health in ("unavailable", "rejected") else "ok",
                       "checked_at": checked, "managed_root": str(self.managed_root), "managed_health": managed_health, "managed_directories": managed_dirs,
                       "total": len(self.items) if complete else None, "observed_total": len(self.items), "count_complete": complete,
                       "queries": queries, "failed_queries": failures, "query_failures": query_failures, "limited": limited,
                       "root_limit": self.ROOT_LIMIT, "item_limit": self.ITEM_LIMIT, "byte_limit": self.BYTE_LIMIT,
                       "cache_seconds": self.CACHE_SECONDS, "association_scope": "loaded_thread_cwd"}

    def snapshot(self):
        return self.result | {"items": [{key: value for key, value in item.items() if not key.startswith("_")} for item in self.items.values()]}

    def detail(self, identity):
        item = self.items.get(identity)
        return ({key: value for key, value in item.items() if not key.startswith("_")} | {"path": item["_path"]}) if item else None
