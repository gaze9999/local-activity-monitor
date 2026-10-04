from contextlib import closing
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.error_records import DiagnosticCollector, tool_error
from local_activity_monitor.server import Dashboard

THREAD = "00000000-0000-4000-8000-000000000001"
TIME = "2026-10-04T03:32:58Z"


class ErrorRecordTests(unittest.TestCase):
    def test_error_projection_never_returns_private_output(self):
        value = tool_error({"isError": True, "error": {"code": "new_provider_error", "message": "PRIVATE_SECRET"}, "content": [{"text": "PRIVATE_PROMPT"}]})
        self.assertEqual(value["code"], "new_provider_error")
        self.assertNotIn("PRIVATE", json.dumps(value))
        self.assertIsNone(tool_error({"status": "ok", "exit_code": 0}))
        self.assertIsNone(tool_error("Example source contains the word error"))
        self.assertEqual(tool_error("Process exited with code 1\nPRIVATE_COMMAND")["exit_code"], 1)
        self.assertIsNone(tool_error("Process exited with code 0"))

    def test_desktop_submit_error_and_warning_are_incremental_and_private(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            root = home/"desktop-logs";root.mkdir()
            path = root/"codex-desktop-fixture.log"
            path.write_text(f'{TIME} error [electron-message-handler] Error submitting steering turn for conversation conversationId={THREAD} errorName=Error errorMessage="PRIVATE_PROMPT conversationId=00000000-0000-4000-8000-000000000099"\n', encoding="utf-8")
            collector = DiagnosticCollector(home);collector.refresh();first = collector.snapshot()
            self.assertEqual(first["events"][0]["thread_id"], THREAD)
            self.assertEqual(first["events"][0]["code"], "message_submit_failed")
            self.assertNotIn("PRIVATE", json.dumps(first))
            collector.refresh();self.assertEqual(collector.snapshot(), first)
            with path.open("a", encoding="utf-8") as stream:
                stream.write(f'{TIME} warning [new-module] future failure errorCode=FUTURE_CODE errorName=TimeoutError\n')
            collector.refresh()
            self.assertEqual(len(collector.events), 2)
            self.assertEqual(collector.events[-1]["severity"], "warning")
            self.assertEqual(collector.events[-1]["code"], "FUTURE_CODE")
            self.assertEqual(collector.events[-1]["error_type"], "TimeoutError")

    def test_diagnostic_event_and_partial_line_limits(self):
        with tempfile.TemporaryDirectory() as directory:
            collector = DiagnosticCollector(Path(directory))
            for index in range(1200):
                collector.desktop_line(f'{TIME} error [new-module] failure errorCode=code{index}')
            self.assertEqual(len(collector.events), 1000)
            self.assertEqual(collector.trimmed, 200)
            root = Path(directory)/"desktop-logs";root.mkdir()
            (root/"codex-desktop-fixture.log").write_bytes(b"PRIVATE"*20000)
            collector.refresh()
            self.assertLessEqual(sum(len(state["buffer"]) for state in collector.files.values()), 8*65536)

    def test_desktop_permission_failure_is_not_reported_as_missing(self):
        with tempfile.TemporaryDirectory() as directory:
            collector = DiagnosticCollector(Path(directory))
            with patch.object(Path, "rglob", side_effect=PermissionError("PRIVATE_PATH")):
                collector.refresh_desktop()
            self.assertEqual(collector.health["desktop"], "unavailable")
            self.assertNotIn("PRIVATE", json.dumps(collector.snapshot()))

    def test_core_schema_check_and_incremental_read_only_projection(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory);path = home/"logs_9.sqlite"
            with closing(sqlite3.connect(path)) as db, db:
                db.execute("CREATE TABLE logs(id INTEGER PRIMARY KEY,ts INTEGER,level TEXT,target TEXT,thread_id TEXT,feedback_log_body TEXT)")
                db.execute("INSERT INTO logs VALUES(1,1791084778,'WARN','codex_core::responses_retry',?,'stream disconnected PRIVATE_SECRET')", (THREAD,))
                db.execute("INSERT INTO logs VALUES(2,1791084778,'INFO','codex_core',?,'PRIVATE_PROMPT')", (THREAD,))
            collector = DiagnosticCollector(home);collector.refresh();collector.refresh()
            self.assertEqual(len(collector.events), 1)
            self.assertEqual(collector.events[0]["code"], "stream_interrupted")
            self.assertNotIn("PRIVATE", json.dumps(collector.snapshot()))
            with closing(sqlite3.connect(path)) as db, db:
                self.assertEqual(db.execute("SELECT count(*) FROM logs").fetchone()[0], 2)
                db.execute("DROP TABLE logs");db.execute("CREATE TABLE logs(future_schema TEXT)")
            collector.refresh();self.assertEqual(collector.health["core"], "unsupported")

    def test_mixed_tool_failure_belongs_to_outer_call(self):
        with tempfile.TemporaryDirectory() as directory:
            collector = CodexCollector(Path(directory));state = collector.new_file(Path(directory)/f"rollout-{THREAD}.jsonl")
            collector.files[Path(directory)/"fixture"] = state
            collector.consume(state, {"timestamp": TIME, "type": "response_item", "payload": {"type": "function_call", "name": "exec", "call_id": "call_1", "arguments": 'await tools.mcp__first__run({}); await tools.mcp__second__run({});'}})
            collector.consume(state, {"timestamp": TIME, "type": "response_item", "payload": {"type": "function_call_output", "call_id": "call_1", "output": {"isError": True, "error": {"code": "outer_failure", "message": "PRIVATE"}}}})
            events = collector.snapshot()["error_events"]
            self.assertEqual(events[0]["category"], "tool")
            self.assertIsNone(events[0]["server"])
            self.assertEqual(events[0]["tool"], "exec")
            self.assertNotIn("PRIVATE", json.dumps(events))
            collector.features["errors"] = False;self.assertEqual(collector.snapshot()["error_events"], [])

    def test_dashboard_errors_switch_and_monitor_errors(self):
        with tempfile.TemporaryDirectory() as directory:
            dashboard = Dashboard(Path(directory), codex=True)
            dashboard.diagnostics.desktop_line(f'{TIME} error [new-module] failure')
            dashboard.refresh();self.assertEqual(len(dashboard.snapshot("all")["errors"]["events"]), 1)
            dashboard.set_settings({"observations": {"errors": False}})
            self.assertEqual(dashboard.snapshot("all")["errors"]["events"], [])
            dashboard.monitor.failed(ValueError("PRIVATE"))
            dashboard.monitor.requested(400)
            value = dashboard.snapshot("all")["errors"]
            self.assertEqual(value["categories"]["monitor"], 2)
            self.assertNotIn("PRIVATE", json.dumps(value))


if __name__ == "__main__":
    unittest.main()
