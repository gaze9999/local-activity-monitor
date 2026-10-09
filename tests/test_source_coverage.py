import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.codex_metadata import CATALOG_LIMIT, INDEX_TAIL_LIMIT, read_metadata
from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.server import Dashboard
from local_activity_monitor.mcp_records import summarize
from local_activity_monitor.thread_state import ThreadState
from local_activity_monitor.history_store import HistoryStore


THREAD = "00000000-0000-0000-0000-000000000001"


class SourceCoverageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def session(self):
        root = self.home / "sessions"
        root.mkdir()
        path = root / ("rollout-" + THREAD + ".jsonl")
        path.write_text(json.dumps({"type": "session_meta", "timestamp": "2026-10-04T00:00:00Z", "payload": {"id": THREAD, "base_instructions": "SECRET_PROMPT"}}) + "\n", encoding="utf-8")
        return path

    def test_metadata_reports_actual_schema_limits_and_failure_without_private_fields(self):
        state = self.home / "state_42.sqlite"
        with closing(sqlite3.connect(state)) as db, db:
            db.execute("CREATE TABLE threads(id TEXT,title TEXT,model TEXT,first_user_message TEXT)")
            db.execute("INSERT INTO threads VALUES(?,?,?,?)", (THREAD, "Fixture", "future-model", "PRIVATE_CONTENT"))
        index = self.home / "session_index.jsonl"
        index.write_text(json.dumps({"id": THREAD, "thread_name": "Fixture", "prompt": "PRIVATE_CONTENT"}) + "\n", encoding="utf-8")
        report = {}
        entries = read_metadata(self.home, {THREAD}, source_info=report)
        self.assertEqual(entries[THREAD]["model"], "future-model")
        self.assertEqual(report[str(state)]["fields"], ["id", "title", "model"])
        self.assertEqual(report[str(state)]["rows_read"], 1)
        self.assertEqual(report[str(state)]["selected_threads"], 1)
        self.assertEqual(report[str(index)]["tail_bytes"], INDEX_TAIL_LIMIT)
        self.assertEqual(report[str(self.home / "sqlite/codex-dev.db")]["health"], "missing")
        self.assertNotIn("PRIVATE_CONTENT", json.dumps([entries, report]))
        self.assertNotIn("first_user_message", json.dumps(report))
        self.assertFalse((self.home / "sqlite/codex-dev.db").exists())
        state_file = self.home / ".codex-global-state.json"
        state_file.write_text("invalid JSON", encoding="utf-8")
        read_metadata(self.home, {THREAD}, source_info=report)
        self.assertEqual(report[str(state_file)]["health"], "unsupported")

    def test_catalog_reports_actual_row_limit_and_read_count(self):
        database = self.home / "sqlite/codex-dev.db"
        database.parent.mkdir()
        with closing(sqlite3.connect(database)) as db, db:
            db.execute("CREATE TABLE local_thread_catalog(host_id TEXT,thread_id TEXT,display_title TEXT,source_created_at TEXT,source_updated_at TEXT,source_kind TEXT,thread_source TEXT,project_id TEXT)")
            db.execute("INSERT INTO local_thread_catalog VALUES(?,?,?,?,?,?,?,?)", ("local", THREAD, "Fixture", None, None, "codex", "user", None))
        report = {}
        read_metadata(self.home, set(), source_info=report)
        self.assertEqual(report[str(database)]["health"], "ok")
        self.assertEqual(report[str(database)]["rows_read"], 1)
        self.assertEqual(report[str(database)]["row_limit"], CATALOG_LIMIT)

    def test_automation_queries_only_return_selected_threads_and_report_each_scope(self):
        database = self.home / "sqlite/codex-dev.db"
        database.parent.mkdir()
        with closing(sqlite3.connect(database)) as db, db:
            db.execute("CREATE TABLE local_thread_catalog(host_id TEXT,thread_id TEXT,display_title TEXT,source_created_at TEXT,source_updated_at TEXT,source_kind TEXT,thread_source TEXT,project_id TEXT)")
            db.execute("INSERT INTO local_thread_catalog VALUES(?,?,?,?,?,?,?,?)", ("local", THREAD, "Fixture", None, None, "codex", "user", None))
            db.execute("CREATE TABLE automations(id TEXT,kind TEXT,target_thread_id TEXT,status TEXT)")
            db.execute("CREATE TABLE automation_runs(thread_id TEXT,automation_id TEXT)")
            db.executemany("INSERT INTO automations VALUES(?,?,?,?)", [("fixture", "heartbeat", THREAD, "ACTIVE"), ("other", "cron", "UNSELECTED", "ACTIVE")])
            db.executemany("INSERT INTO automation_runs VALUES(?,?)", [(THREAD, "fixture")]*3 + [("UNSELECTED", "other")]*1000)
        report = {}
        entries = read_metadata(self.home, set(), source_info=report)
        self.assertEqual(entries[THREAD]["trigger"], "heartbeat")
        self.assertTrue(entries[THREAD]["has_schedule"])
        self.assertNotIn("UNSELECTED", entries)
        queries = report[str(database)]["queries"]
        self.assertEqual([query["rows_read"] for query in queries], [1, 1])
        self.assertTrue(all(query["selected_threads"] == query["row_limit"] == 1 for query in queries))
        self.assertEqual(queries[0]["fields"], ["automation_runs.thread_id", "automations.kind"])
        self.assertEqual(report[str(database)]["rows_read"], 1)

    def test_mcp_summary_keeps_infrequent_source_evidence_outside_table_limit(self):
        rare = {"server": "rare", "tool": "recording_status", "timestamp": "2026-10-04T00:00:00Z", "completed_at": "2026-10-04T00:00:01Z", "nested": False, "result": {"recording_enabled": False}, "duration_ms": 1000}
        busy = rare | {"server": "busy", "tool": "run", "timestamp": "2026-10-04T00:00:02Z", "completed_at": "2026-10-04T00:00:03Z", "result": {}}
        events = [rare | {"thread_id": THREAD, "call_id": "rare-call"}] + [busy | {"thread_id": THREAD, "call_id": "busy-"+str(index)} for index in range(1000)]
        dashboard = Dashboard(self.home, codex=True)
        with patch.object(dashboard.codex, "snapshot", return_value={"health": "ok", "threads": [], "tools": {}, "mcp_events": events}):
            dashboard.refresh()
        mcp = dashboard.snapshot("all")["mcp"]
        source = next(source for source in mcp["servers"] if source["server"] == "rare")
        self.assertEqual(len(mcp["events"]), 1000)
        self.assertTrue(all(event["server"] == "busy" for event in mcp["events"]))
        self.assertEqual(source["calls"], 1)
        self.assertEqual(source["tools"], ["recording_status"])
        self.assertEqual(source["connection"]["state"], "response")
        self.assertFalse(mcp["recording_status"]["rare"]["enabled"])
        failure = {"server": "rare", "timestamp": "2026-10-04T00:00:04Z", "code": "connection_closed"}
        source = next(source for source in summarize({}, events, diagnostics=[failure])["servers"] if source["server"] == "rare")
        self.assertEqual(source["connection"]["state"], "error")
        self.assertEqual(source["connection"]["last_response_at"], rare["completed_at"])

    def test_checkpoint_health_distinguishes_empty_data_from_read_failures(self):
        path = self.home / "checkpoint.json"
        self.assertEqual(ThreadState(path).load_health, "missing")
        for raw, expected in [(b"invalid JSON", "unsupported"), (b"[]", "unsupported"), (b"x"*(ThreadState.BYTE_LIMIT+1), "oversized"), (b'{"version":1,"entries":{},"skills":[]}', "ok")]:
            with self.subTest(expected=expected):
                path.write_bytes(raw)
                state = ThreadState(path)
                self.assertEqual(state.load_health, expected)
                self.assertEqual(state.skills, [])
        with patch.object(HistoryStore, "connect", side_effect=OSError("Fixture unreadable")):
            self.assertEqual(ThreadState(path).load_health, "unavailable")
        with patch.object(Path, "is_symlink", return_value=True):
            state = ThreadState(path)
            self.assertEqual(state.load_health, "unavailable")
            state.update([])
            self.assertEqual(state.write_health, "unavailable")
        path = self.home / "invalid/checkpoint.json"
        path.parent.mkdir()
        path.write_bytes(b"invalid JSON")
        state = ThreadState(path)
        state.update([])
        self.assertEqual(state.load_health, "unsupported")
        self.assertEqual(state.write_health, "ok")
        self.assertEqual(ThreadState(path).load_health, "ok")

    def test_registry_uses_loaded_reader_results_and_generic_telemetry_without_io(self):
        path = self.session()
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        snapshot = dashboard.snapshot("all")
        checkpoint = snapshot['sources']['thread_state']['readers']
        self.assertEqual({reader['name']: reader['checked_at'] for reader in checkpoint}, {
            'Checkpoint load': dashboard.codex.thread_state.load_checked_at,
            'Checkpoint save': dashboard.codex.thread_state.write_checked_at,
        })
        self.assertTrue(all(reader['checked_at'] for reader in checkpoint))
        self.assertTrue(next(reader for reader in snapshot['sources']['catalog']['readers'] if reader['name']=='subagent_lifecycle')['checked_at'])
        checked = dashboard.activity_history.checked_at
        self.assertEqual(snapshot['sources']['activity_history']['checked_at'], checked)
        snapshot['updated_at'] = '2099-01-01T00:00:00Z'
        self.assertEqual(dashboard.sources(snapshot)['activity_history']['checked_at'], checked)
        session = snapshot["sources"]["session"]
        self.assertIn(str(path), session["locations"])
        self.assertIn("git", session["features"])
        self.assertNotIn("file_limit", session["limits"])
        self.assertEqual(session["limits"]["file_count"], 1)
        self.assertEqual(session["limits"]["read_limit"], CodexCollector.READ_LIMIT)
        self.assertNotIn("SECRET_PROMPT", json.dumps(snapshot))
        self.assertFalse(any("*" in location for source in snapshot["sources"].values() for location in source["locations"]))
        snapshot["mcp"]["telemetry"]["future_service"] = {"health": "ok", "window": "all", "summary": {"credits": 0}, "recent": [{"id": "fixture"}], "series": [], "reader": {"locations": ["fixture.sqlite"], "record_limit": 17}}
        with patch("local_activity_monitor.server.monitor_config", side_effect=AssertionError("Unexpected source reread")), patch.object(Path, "open", side_effect=AssertionError("Unexpected file read")):
            source = dashboard.sources(snapshot)["telemetry:future_service"]
        self.assertEqual(source["locations"], ["fixture.sqlite"])
        self.assertEqual(source["limits"]["record_limit"], 17)
        self.assertEqual(source["limits"]["loaded_records"], 1)
        self.assertEqual(source["limits"]["series_points"], 0)
        self.assertEqual(source["fields"], ["credits"])
        self.assertEqual(source["window"], "all")
        self.assertEqual(source["scope"], "reported_telemetry")
        dashboard.set_settings({"observations": {"codex": False}})
        disabled = dashboard.snapshot("all")["sources"]
        self.assertEqual(disabled["session"]["health"], "disabled")
        self.assertEqual(disabled["catalog"]["health"], "disabled")
        self.assertEqual(disabled["diagnostics"]["health"], "disabled")
        self.assertNotIn("file_count", disabled["session"]["limits"])

    def test_paused_telemetry_preserves_coverage_without_reading_the_database(self):
        (self.home / "config.toml").write_text("[mcp_servers.jev]\nenabled = true\n", encoding="utf-8")
        dashboard = Dashboard(self.home)
        dashboard.observations["mcp"] = False
        with patch.object(dashboard.jev, "snapshot", side_effect=AssertionError("Paused reader should not query")):
            dashboard.refresh()
        snapshot = dashboard.snapshot("all")
        self.assertEqual(snapshot["mcp"]["telemetry"]["jev"]["health"], "paused")
        self.assertEqual(snapshot["sources"]["telemetry:jev"]["health"], "paused")
        self.assertIsNone(snapshot["sources"]["telemetry:jev"]["window"])


if __name__ == "__main__":
    unittest.main()
