from contextlib import closing
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from local_activity_monitor.api_records import api_diagnostic, api_summary
from local_activity_monitor.error_records import DiagnosticCollector
from local_activity_monitor.server import Dashboard


THREAD = "00000000-0000-4000-8000-000000000001"


class ApiRecordTests(unittest.TestCase):
    def test_request_fields_zero_missing_and_privacy(self):
        body = f'event.name="codex.api_request" provider=OpenAI model=gpt-fixture endpoint="/responses" success="true" status_code=200 duration_ms=0 input_tokens=0 request_id=req_fixture thread_id={THREAD} errorMessage="PRIVATE input_tokens=999" authorization="PRIVATE"'
        event = api_diagnostic("codex_otel.log_only", body)
        self.assertEqual(event["input_tokens"], 0)
        self.assertEqual(event["duration_ms"], 0)
        self.assertEqual(event["status"], "returned")
        self.assertNotIn("output_tokens", event)
        self.assertNotIn("PRIVATE", json.dumps(event))
        self.assertEqual(event["thread_id"], THREAD)

    def test_non_model_requests_and_quoted_spoof_are_ignored(self):
        self.assertIsNone(api_diagnostic("codex_otel.log_only", 'event.name="codex.api_request" endpoint="/models"'))
        self.assertIsNone(api_diagnostic("other", 'event.name="codex.api_request"'))
        self.assertIsNone(api_diagnostic("codex_api::endpoint::responses_websocket", 'errorMessage="connecting to websocket"'))
        self.assertIsNone(api_diagnostic("codex_otel.log_only", 'message="event.name=codex.api_request"'))
        self.assertIsNone(api_diagnostic("codex_otel.log_only", 'message="PRIVATE event.name=codex.api_request'))

    def test_websocket_send_and_connections_are_separate(self):
        sent = api_diagnostic("codex_otel.log_only", 'event.name="codex.websocket.request" success="true" duration_ms=3')
        connected = api_diagnostic("codex_api::endpoint::responses_websocket", 'successfully connected to websocket: PRIVATE_URL model=gpt-fixture')
        retry = api_diagnostic("codex_core::responses_retry", 'stream disconnected retries=1 max_retries=5 sampling_error="PRIVATE" input_tokens=100')
        self.assertEqual(sent["status"], "sent")
        self.assertNotIn("duration_ms", connected)
        self.assertNotIn("input_tokens", retry)
        events = [event | {"timestamp": "2026-10-08T00:00:00Z", "observation_id": str(index)} for index, event in enumerate((sent, connected, retry))]
        summary = api_summary(events+events)
        self.assertEqual((summary["requests"], summary["connections"], summary["retry_events"]), (1, 1, 1))

    def test_invalid_numbers_and_secret_identifier_are_not_projected(self):
        event = api_diagnostic("codex_otel.log_only", 'event.name="codex.api_request" success="false" duration_ms=-1 input_tokens=NaN output_tokens=1.5 total_tokens=9999999999999999 status_code=429 request_id=sk-PRIVATE thread_id=PRIVATE')
        self.assertEqual(event["status"], "failed")
        self.assertEqual(event["http_status"], 429)
        for key in ("duration_ms", "input_tokens", "output_tokens", "total_tokens", "request_id", "thread_id"):
            self.assertNotIn(key, event)

    def test_core_incremental_metadata_and_independent_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            path = home / "logs_10.sqlite"
            stamp = datetime.now(timezone.utc).timestamp()
            with closing(sqlite3.connect(path)) as db, db:
                db.execute("CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,feedback_log_body TEXT)")
                db.execute("INSERT INTO logs VALUES(1,?,'INFO','codex_otel.log_only',?)", (stamp, 'event.name="codex.api_request" endpoint="/responses" status_code=200 request_id=req_fixture input_tokens=0 body="PRIVATE"'))
            collector = DiagnosticCollector(home)
            collector.capture_logs = collector.capture_sql = False
            collector.refresh()
            collector.refresh()
            self.assertEqual(len(collector.api_events), 1)
            self.assertFalse(collector.logs)
            self.assertNotIn("PRIVATE", json.dumps(list(collector.api_events)))
            collector.capture_api = False
            with closing(sqlite3.connect(path)) as db, db:
                db.execute("INSERT INTO logs VALUES(2,?,'INFO','codex_otel.log_only',?)", (stamp+1, 'event.name="codex.websocket.request" success="true"'))
            collector.refresh()
            self.assertEqual(len(collector.api_events), 1)

    def test_dashboard_scope_disable_reenable_and_source_read_only(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            path = home / "logs_10.sqlite"
            with closing(sqlite3.connect(path)) as db, db:
                db.execute("CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)")
                db.execute("INSERT INTO logs VALUES(1,?,'INFO','codex_otel.log_only',?)", (datetime.now(timezone.utc).timestamp(), 'event.name="codex.api_request" endpoint="/responses" status_code=500'))
            original = path.read_bytes()
            dashboard = Dashboard(home, True)
            dashboard.refresh()
            self.assertEqual(dashboard.snapshot("1h")["model_api"]["failed_requests"], 1)
            dashboard.set_settings({"observations": {"model_api": False}})
            self.assertFalse(dashboard.snapshot("all")["model_api"]["enabled"])
            self.assertFalse(dashboard.diagnostics.api_events)
            dashboard.set_settings({"observations": {"model_api": True}})
            self.assertEqual(dashboard.snapshot("all")["model_api"]["requests"], 1)
            self.assertEqual(path.read_bytes(), original)

    def test_desktop_replay_and_bounded_history(self):
        with tempfile.TemporaryDirectory() as directory:
            collector = DiagnosticCollector(Path(directory))
            line = f'2026-10-08T00:00:00Z info [codex_otel.log_only] event.name="codex.api_request" endpoint="/responses" thread_id={THREAD}'
            collector.desktop_line(line, "fixture.log", offset=0)
            collector.desktop_line(line, "fixture.log", offset=0)
            self.assertEqual(len(collector.api_events), 1)
            context = {"timestamp": "2026-10-08T00:00:01Z", "source": "codex_core", "module": "codex_otel.log_only", "file": "fixture.sqlite"}
            for identity in range(1005):
                collector.observe_api(context["module"], 'event.name="codex.api_request"', context | {"record_id": identity})
            self.assertEqual(len(collector.api_events), 1000)
            self.assertFalse(any(event["timestamp"].endswith("00Z") for event in collector.api_events))
