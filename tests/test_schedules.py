"""Schedule projection, privacy and bounded source checks."""
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from local_activity_monitor.codex_schedules import read_schedules, SCHEDULE_LIMIT, RUN_LIMIT
from local_activity_monitor.collectors import CodexCollector


class ScheduleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.path = self.home/"sqlite/codex-dev.db"
        self.path.parent.mkdir()

    def database(self):
        return closing(sqlite3.connect(self.path))

    def test_projection_keeps_metadata_and_excludes_private_content(self):
        with self.database() as db, db:
            db.execute("CREATE TABLE automations(id,name,status,kind,next_run_at,last_run_at,rrule,target_thread_id,prompt,account_id,cwds)")
            db.execute("INSERT INTO automations VALUES(?,?,?,?,?,?,?,?,?,?,?)", ("schedule-1", "Fixture", "ACTIVE", "heartbeat", 1791200000000, 1791100000, "FREQ=DAILY;BYHOUR=7", "a"*36, "PRIVATE_PROMPT", "PRIVATE_ACCOUNT", "PRIVATE_ROOT"))
            db.execute("CREATE TABLE automation_runs(automation_id,thread_id,created_at,status,inbox_summary,archived_assistant_message)")
            db.execute("INSERT INTO automation_runs VALUES(?,?,?,?,?,?)", ("schedule-1", "b"*36, 1791100000000, "completed", "PRIVATE_SUMMARY", "PRIVATE_MESSAGE"))
        report = {}
        result = read_schedules(self.home, report)
        self.assertEqual(result["health"], "ok")
        item = result["items"][0]
        self.assertEqual(item["name"], "Fixture")
        self.assertEqual(item["next_run_at"], "2026-10-05T11:33:20.000Z")
        self.assertEqual(item["last_thread_id"], "b"*36)
        self.assertEqual(item["last_run_status"], "completed")
        self.assertNotIn("PRIVATE", json.dumps([result, report]))
        self.assertEqual(next(iter(report.values()))["runs_read"], 1)
        with self.database() as db:
            self.assertEqual(db.execute("SELECT prompt FROM automations").fetchone()[0], "PRIVATE_PROMPT")

    def test_old_schema_and_missing_optional_fields(self):
        with self.database() as db, db:
            db.execute("CREATE TABLE automations(id,status)")
            db.execute("INSERT INTO automations VALUES('old','PAUSED')")
        result = read_schedules(self.home)
        self.assertEqual(result["health"], "ok")
        self.assertEqual(result["items"][0]["status"], "PAUSED")
        self.assertIsNone(result["items"][0]["next_run_at"])

    def test_missing_and_unsupported_sources_are_distinct(self):
        self.assertEqual(read_schedules(self.home)["health"], "missing")
        with self.database() as db, db:
            db.execute("CREATE TABLE other(value)")
        self.assertEqual(read_schedules(self.home)["health"], "unsupported")

    def test_source_queries_remain_bounded(self):
        with self.database() as db, db:
            db.execute("CREATE TABLE automations(id,status,next_run_at)")
            db.executemany("INSERT INTO automations VALUES(?,'ACTIVE',?)", [(f"s-{i}", i+1791200000000) for i in range(SCHEDULE_LIMIT+25)])
            db.execute("CREATE TABLE automation_runs(automation_id,thread_id,created_at)")
            db.executemany("INSERT INTO automation_runs VALUES('s-1',?,?)", [(str(i), i) for i in range(RUN_LIMIT+25)])
        report = {}
        result = read_schedules(self.home, report)
        self.assertEqual(len(result["items"]), SCHEDULE_LIMIT)
        self.assertEqual(next(iter(report.values()))["runs_read"], RUN_LIMIT)

    def test_collector_reuses_schedule_metadata_and_respects_disabled_feature(self):
        (self.home/"sessions").mkdir()
        with self.database() as db, db:
            db.execute("CREATE TABLE automations(id,status)")
            db.execute("INSERT INTO automations VALUES('fixture','ACTIVE')")
        collector = CodexCollector(self.home/"sessions")
        collector.refresh()
        cache = {}
        first = collector.snapshot(metadata_cache=cache)
        second = collector.snapshot(metadata_cache=cache)
        self.assertEqual(first["schedules"], second["schedules"])
        self.assertEqual(first["schedules"]["items"][0]["id"], "fixture")
        collector.features["metadata"] = False
        self.assertEqual(collector.snapshot()["schedules"]["health"], "disabled")


if __name__ == "__main__":
    unittest.main()
