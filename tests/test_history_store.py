"""SQLite migration, atomicity and ownership fixtures."""
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor import history_store
from local_activity_monitor.activity_history import ActivityHistory
from local_activity_monitor.error_history import ErrorHistory
from local_activity_monitor.thread_state import ThreadState


class HistoryStoreTests(unittest.TestCase):
    def test_batch_aggregates_share_snapshot_and_keep_results_independent(self):
        self.value['mcp'] = [{'timestamp':'2026-10-10T00:00:00Z', 'server':'included', 'call_id':'first', 'nested':False},
                             {'timestamp':'2026-10-10T00:00:00Z', 'server':'excluded', 'call_id':'second'}]
        self.store.save(json.dumps(self.value))
        aggregate = self.store._aggregate
        calls = 0
        def changed_source(db, where, args):
            nonlocal calls
            result = aggregate(db, where, args)
            calls += 1
            if calls == 1:
                self.store.save(json.dumps(self.value|{'mcp':[{'timestamp':'2026-10-10T00:01:00Z', 'server':'included', 'call_id':'third'}]}))
            return result
        with patch.object(self.store, '_aggregate', side_effect=changed_source):
            results = self.store.aggregate_many('mcp', [None, '2026-10-10T00:00:00Z'], ['excluded'])
        self.assertEqual([result['total'] for result in results], [1, 1])
        self.assertEqual(self.store.aggregate('mcp', excluded_sources=['excluded'])['total'], 2)
        results[0]['servers']['included']['total'] = 99
        self.assertEqual(results[1]['servers']['included']['total'], 1)
        self.assertEqual(self.store.aggregate_many('mcp', ['2026-10-11T00:00:00Z'])[0]['total'], 0)

    def test_batch_aggregate_validates_ranges_even_without_database(self):
        store = history_store.HistoryStore(self.root/'missing'/'activity-history.json', 'activity')
        self.assertEqual([result['total'] for result in store.aggregate_many('sql', [None, '2026-10-10T00:00:00Z'])], [0, 0])
        for boundaries in ([], [None]*17, ['2026-10-10'], [123]):
            with self.subTest(boundaries=boundaries), self.assertRaises(ValueError):
                store.aggregate_many('sql', boundaries)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="LAM SQLite ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.store = history_store.HistoryStore(self.root / "activity-history.json", "activity")
        self.value = {"version": 3, "retention_days": 7, "sql": [], "web": [], "mcp": []}
        self.store.save(json.dumps(self.value))

    def test_namespaces_share_database_without_overwriting_each_other(self):
        errors = history_store.HistoryStore(self.root / "error-history.json", "errors")
        errors.save(json.dumps({"version": 1, "events": []}))
        threads = history_store.HistoryStore(self.root / "thread-state.json", "threads")
        threads.save(json.dumps({"version": 1, "entries": {}, "skills": []}))
        self.assertEqual(self.store.read_document(1024), self.value)
        self.assertEqual(threads.read_document(1024)["entries"], {})
        with closing(sqlite3.connect(self.store.path)) as db:
            self.assertEqual(db.execute("SELECT count(*) FROM history_state").fetchone()[0], 3)
            self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], history_store.SCHEMA_VERSION)

    def test_schema_upgrade_backs_up_and_preserves_data(self):
        self.value["mcp"] = [{"timestamp": "2026-10-08T00:00:00Z", "input_tokens": 0, "cache_hit": False, "missing": None}]
        self.store.save(json.dumps(self.value))
        current = history_store.SCHEMA_VERSION
        changes = {**history_store.MIGRATIONS, current+1: ("ALTER TABLE history_state ADD COLUMN extra TEXT",)}
        with patch.object(history_store, "SCHEMA_VERSION", current+1), patch.object(history_store, "MIGRATIONS", changes):
            self.assertEqual(self.store.read_document(2048), self.value)
            with closing(sqlite3.connect(self.store.path)) as db:
                self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], current+1)
                self.assertEqual(db.execute("SELECT extra FROM history_state").fetchone()[0], None)
        backup = self.store.path.with_name(self.store.path.name + f".schema-v{current}.bak")
        with closing(sqlite3.connect(backup)) as db:
            self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], current)
            self.assertEqual(db.execute("SELECT count(*) FROM history_items").fetchone()[0], 1)

    def test_failed_schema_upgrade_rolls_back_columns_data_and_version(self):
        current = history_store.SCHEMA_VERSION
        changes = {**history_store.MIGRATIONS, current+1: ("ALTER TABLE history_state ADD COLUMN extra TEXT", "INVALID SQL")}
        with patch.object(history_store, "SCHEMA_VERSION", current+1), patch.object(history_store, "MIGRATIONS", changes):
            with self.assertRaises(sqlite3.Error):
                self.store.read_document(1024)
        self.assertEqual(self.store.read_document(1024), self.value)
        with closing(sqlite3.connect(self.store.path)) as db:
            self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], current)
            self.assertNotIn("extra", [row[1] for row in db.execute("PRAGMA table_info(history_state)")])

    def test_schema_rebuild_removes_column_and_converts_type_without_losing_rows(self):
        with closing(sqlite3.connect(self.store.path)) as db:
            db.execute("ALTER TABLE history_state ADD COLUMN obsolete TEXT")
        current = history_store.SCHEMA_VERSION
        changes = {**history_store.MIGRATIONS, current+1: (
            "CREATE TABLE replacement (namespace TEXT PRIMARY KEY, metadata TEXT NOT NULL, updated_at TEXT NOT NULL)",
            "INSERT INTO replacement SELECT namespace, metadata, CAST(updated_at AS TEXT) FROM history_state",
            "DROP TABLE history_state", "ALTER TABLE replacement RENAME TO history_state",
        )}
        with patch.object(history_store, "SCHEMA_VERSION", current+1), patch.object(history_store, "MIGRATIONS", changes):
            self.assertEqual(self.store.read_document(1024), self.value)
            self.store.save(json.dumps(self.value))
            with closing(sqlite3.connect(self.store.path)) as db:
                columns = {row[1]: row[2] for row in db.execute("PRAGMA table_info(history_state)")}
                self.assertNotIn("obsolete", columns)
                self.assertEqual(columns["updated_at"], "TEXT")

    def test_busy_writer_preserves_committed_history(self):
        with closing(sqlite3.connect(self.store.path)) as locked:
            locked.execute("BEGIN IMMEDIATE")
            with self.assertRaises(sqlite3.OperationalError):
                self.store.save(json.dumps({**self.value, "retention_days": 14}))
            locked.rollback()
        self.assertEqual(self.store.read_document(1024), self.value)

    def test_unsupported_version_and_foreign_database_are_preserved(self):
        with closing(sqlite3.connect(self.store.path)) as db:
            db.execute("PRAGMA user_version=99")
        before = self.store.path.read_bytes()
        cache = ActivityHistory(self.store.legacy)
        cache.update([], [])
        self.assertEqual(cache.health, "unavailable")
        self.assertEqual(self.store.path.read_bytes(), before)
        other = self.root / "foreign"
        other.mkdir()
        path = other / "lam-history.sqlite3"
        with closing(sqlite3.connect(path)) as db:
            db.execute("CREATE TABLE unrelated(data TEXT)")
        before = path.read_bytes()
        ErrorHistory(other / "error-history.json").update([])
        self.assertEqual(path.read_bytes(), before)

    def test_oversized_and_invalid_legacy_files_are_preserved(self):
        for content in (b"invalid", b" " * (ThreadState.BYTE_LIMIT + 1)):
            with tempfile.TemporaryDirectory() as folder:
                path = Path(folder) / "thread-state.json"
                path.write_bytes(content)
                cache = ThreadState(path)
                self.assertFalse(cache.entries)
                self.assertEqual(path.read_bytes(), content)
                self.assertFalse(cache.path.exists())

    def test_linked_database_is_rejected_without_touching_target(self):
        with tempfile.TemporaryDirectory() as folder:
            target = Path(folder) / "database"
            target.write_bytes(b"keep")
            link = self.root / "linked"
            link.mkdir()
            try:
                (link / "lam-history.sqlite3").symlink_to(target)
            except OSError:
                self.skipTest("Symlink creation unavailable")
            cache = ErrorHistory(link / "error-history.json")
            cache.update([])
            self.assertEqual(cache.health, "unavailable")
            self.assertEqual(target.read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
