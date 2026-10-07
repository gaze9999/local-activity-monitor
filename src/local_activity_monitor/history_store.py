"""Versioned SQLite storage for bounded, projected history and checkpoints."""
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import stat
import time


APPLICATION_ID = 0x4C414D31
SCHEMA_VERSION = 1
MIGRATIONS = {1: (
    "CREATE TABLE history_state (namespace TEXT PRIMARY KEY, metadata TEXT NOT NULL, updated_at INTEGER NOT NULL)",
    "CREATE TABLE history_items (namespace TEXT NOT NULL, section TEXT NOT NULL, item_key TEXT NOT NULL, ordinal INTEGER NOT NULL, event_time TEXT, payload TEXT NOT NULL, PRIMARY KEY(namespace, section, item_key))",
    "CREATE INDEX history_time ON history_items(namespace, section, event_time)",
)}
SECTIONS = {"activity": {"sql": list, "web": list, "mcp": list},
            "errors": {"events": list}, "threads": {"entries": dict, "skills": list}}


def unsafe(path):
    if path.is_symlink():
        return True
    try:
        return bool(getattr(path.lstat(), "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))
    except FileNotFoundError:
        return False


class HistoryStore:
    def __init__(self, legacy, namespace):
        self.legacy = Path(legacy)
        self.path = self.legacy.with_name("lam-history.sqlite3")
        self.namespace = namespace
        self.sections = SECTIONS[namespace]
        self.imported = False

    def check_paths(self):
        for path in (self.path.parent, self.path, *(self.path.with_name(self.path.name + suffix) for suffix in ("-journal", "-wal", "-shm"))):
            if unsafe(path):
                raise OSError("Unsafe history storage")

    def connect(self, write=False):
        self.check_paths()
        if write:
            self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            self.check_paths()
        db = sqlite3.connect(self.path.resolve().as_uri() + ("?mode=rwc" if write else "?mode=rw"), uri=True, timeout=.2)
        try:
            application = db.execute("PRAGMA application_id").fetchone()[0]
            version = db.execute("PRAGMA user_version").fetchone()[0]
            if write and application == 0 and version == 0:
                if db.execute("SELECT 1 FROM sqlite_master LIMIT 1").fetchone():
                    raise sqlite3.DatabaseError("Unrecognized history database")
                self.migrate(db, 0)
                application, version = APPLICATION_ID, SCHEMA_VERSION
                if os.name != "nt":
                    self.path.chmod(0o600)
            if application == APPLICATION_ID and 0 < version < SCHEMA_VERSION:
                self.migrate(db, version)
                version = SCHEMA_VERSION
            if application != APPLICATION_ID or version != SCHEMA_VERSION:
                raise sqlite3.DatabaseError("Unsupported history database version")
            return db
        except BaseException:
            db.close()
            raise

    def migrate(self, db, version):
        if any(target not in MIGRATIONS for target in range(version + 1, SCHEMA_VERSION + 1)):
            raise sqlite3.DatabaseError("Missing history migration")
        if version:
            # Preserve an existing complete backup after a failed upgrade/retry.
            backup = self.path.with_name(self.path.name + ".schema-v" + str(version) + ".bak")
            if unsafe(backup):
                raise OSError("Unsafe history backup")
            if not backup.exists():
                with backup.open("xb"):
                    pass
                if os.name != "nt":
                    backup.chmod(0o600)
                with closing(sqlite3.connect(backup)) as target:
                    db.backup(target)
            with closing(sqlite3.connect(backup.resolve().as_uri() + "?mode=ro", uri=True)) as saved:
                if saved.execute("PRAGMA application_id").fetchone()[0] != APPLICATION_ID or saved.execute("PRAGMA user_version").fetchone()[0] != version or saved.execute("PRAGMA quick_check").fetchone()[0] != "ok":
                    raise sqlite3.DatabaseError("Invalid history migration backup")
        with db:
            db.execute("BEGIN IMMEDIATE")
            for target in range(version + 1, SCHEMA_VERSION + 1):
                for statement in MIGRATIONS[target]:
                    db.execute(statement)
            db.execute("PRAGMA application_id=" + str(APPLICATION_ID))
            db.execute("PRAGMA user_version=" + str(SCHEMA_VERSION))

    def load(self, byte_limit):
        self.check_paths()
        if self.path.exists():
            with closing(self.connect()) as db:
                row = db.execute("SELECT metadata FROM history_state WHERE namespace=?", (self.namespace,)).fetchone()
                if row:
                    if not isinstance(row[0], str) or len(row[0]) > 65536:
                        raise ValueError("Invalid history state")
                    value = json.loads(row[0])
                    if not isinstance(value, dict):
                        raise ValueError("Invalid history state")
                    value.update({name: kind() for name, kind in self.sections.items()})
                    total = len(row[0])
                    rows = db.execute("SELECT section, item_key, payload FROM history_items WHERE namespace=? ORDER BY ordinal LIMIT 4001", (self.namespace,))
                    for index, (section, key, payload) in enumerate(rows):
                        if index >= 4000 or section not in self.sections or not isinstance(payload, str):
                            raise ValueError("Invalid history items")
                        total += len(payload)
                        if total > byte_limit:
                            raise ValueError("Oversized history")
                        item = json.loads(payload)
                        if self.sections[section] is dict:
                            value[section][key] = item
                        else:
                            value[section].append(item)
                    return json.dumps(value, ensure_ascii=True, separators=(",", ":")).encode()
        if self.legacy == self.path:
            return None
        if unsafe(self.legacy):
            raise OSError("Unsafe legacy history")
        if self.legacy.exists():
            with self.legacy.open("rb") as stream:
                raw = stream.read(byte_limit + 1)
            self.imported = True
            return raw
        return None

    def save(self, raw):
        value = json.loads(raw)
        metadata = {key: item for key, item in value.items() if key not in self.sections}
        rows = []
        for section, kind in self.sections.items():
            items = value.get(section, kind())
            if not isinstance(items, kind):
                raise ValueError("Invalid history section")
            entries = items.items() if kind is dict else enumerate(items)
            for ordinal, (key, item) in enumerate(entries):
                rows.append((self.namespace, section, str(key), ordinal, item.get("timestamp", item.get("task_time")),
                             json.dumps(item, ensure_ascii=True, separators=(",", ":"))))
        with closing(self.connect(write=True)) as db, db:
            db.execute("INSERT INTO history_state VALUES (?, ?, ?) ON CONFLICT(namespace) DO UPDATE SET metadata=excluded.metadata, updated_at=excluded.updated_at",
                       (self.namespace, json.dumps(metadata, separators=(",", ":")), time.time_ns()))
            db.execute("DELETE FROM history_items WHERE namespace=?", (self.namespace,))
            db.executemany("INSERT INTO history_items VALUES (?, ?, ?, ?, ?, ?)", rows)

    def read_document(self, byte_limit):
        return json.loads(self.load(byte_limit))
