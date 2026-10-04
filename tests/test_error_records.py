from contextlib import closing
from datetime import datetime, timedelta, timezone
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
    def test_sql_diagnostics_desktop_core_and_reenable(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory);path = home/'logs_10.sqlite'
            stamp = datetime.now(timezone.utc).timestamp()
            with closing(sqlite3.connect(path)) as db, db:
                db.execute('CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,feedback_log_body TEXT)')
                db.execute('INSERT INTO logs VALUES(1,?,\'DEBUG\',\'sqlx::query\',?)', (stamp, 'summary="SELECT PRIVATE" elapsed=2.5ms rows_returned=4'))
                db.execute('INSERT INTO logs VALUES(2,?,\'INFO\',\'codex_core\',?)', (stamp, 'SELECT PRIVATE elapsed=999ms'))
            dashboard = Dashboard(home, codex=True);dashboard.refresh()
            events = dashboard.snapshot('all')['codex']['sqlite']['events']
            self.assertEqual(len(events), 1)
            self.assertEqual(events[0]['duration_ms'], 2.5)
            self.assertEqual(events[0]['rows_returned'], 4)
            self.assertNotIn('PRIVATE', json.dumps(events))
            dashboard.diagnostics.desktop_line(f'{TIME} trace [rusqlite] PRIVATE elapsed_secs=0.01')
            self.assertEqual(dashboard.diagnostics.sql_events[-1]['duration_ms'], 10)
            dashboard.set_settings({'observations': {'sqlite': False}})
            with closing(sqlite3.connect(path)) as db, db:
                db.execute('INSERT INTO logs VALUES(3,?,\'ERROR\',\'sqlite\',?)', (stamp, 'summary="UPDATE PRIVATE" rows_affected=1'))
            dashboard.refresh()
            self.assertEqual(dashboard.snapshot('all')['codex']['sqlite']['events'], [])
            dashboard.set_settings({'observations': {'sqlite': True}})
            events = dashboard.snapshot('all')['codex']['sqlite']['events']
            self.assertEqual(len(events), 2)
            self.assertTrue(any(event['result'] == 'log_error' for event in events))

    def test_session_backfills_failures_without_overwriting_latest_context(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory);path = root/f'rollout-{THREAD}.jsonl'
            stamp = datetime.now(timezone.utc).isoformat()
            old = (datetime.now(timezone.utc)-timedelta(hours=25)).isoformat()
            def record(kind, payload, date=stamp):return json.dumps({'timestamp': date, 'type': kind, 'payload': payload})+'\n'
            content = record('session_meta', {'id': THREAD}, old)
            content += record('event_msg', {'type': 'turn_failed', 'error_code': 'historical_failure'})
            content += record('response_item', {'type': 'function_call_output', 'call_id': 'call_old', 'output': {'exit_code': 2, 'message': 'PRIVATE'}})
            content += record('event_msg', {'type': 'token_count', 'info': {'total_token_usage': {'input_tokens': 99}}})
            content += record('event_msg', {'type': 'ignored', 'message': 'PRIVATE'*60})*800
            content += record('event_msg', {'type': 'token_count', 'info': {'total_token_usage': {'input_tokens': 123}}})
            content += record('event_msg', {'type': 'task_started', 'turn_id': 'turn_current'})
            path.write_text(content, encoding='utf-8')
            collector = CodexCollector(root, tail_bytes=65536)
            for _ in range(4):collector.refresh()
            state = collector.files[path]
            self.assertEqual(state['tokens']['input_tokens'], 123)
            self.assertIsNotNone(state['task_start'])
            self.assertEqual({row['code'] for row in state['errors']}, {'historical_failure', 'process_exit'})
            self.assertFalse(state.get('error_cursor'))
            self.assertNotIn('PRIVATE', json.dumps(collector.snapshot()))

    def test_desktop_backfills_24h_errors_before_initial_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory);root = home/'desktop-logs';root.mkdir()
            stamp = datetime.now(timezone.utc).isoformat()
            expired = (datetime.now(timezone.utc)-timedelta(hours=25)).isoformat()
            path = root/'codex-desktop-fixture.log'
            path.write_text(f'{expired} error [mcp] OLD_ERROR\n{stamp} error [mcp] PRIVATE errorCode=HISTORICAL\n'+(f'{stamp} info [module] '+('PRIVATE'*20)+'\n')*5000, encoding='utf-8')
            collector = DiagnosticCollector(home)
            for _ in range(6):collector.refresh()
            self.assertTrue(any(row['code'] == 'HISTORICAL' for row in collector.events))
            self.assertFalse(any(row['timestamp'].startswith(expired[:19]) for row in collector.events))
            self.assertEqual(collector.backfill_pending['desktop'], 0)
            self.assertNotIn('PRIVATE', json.dumps(collector.snapshot()))
            before = collector.read_bytes;collector.refresh();self.assertEqual(collector.read_bytes, before)

    def test_core_backfill_and_forward_paging_do_not_skip_records(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory);path=home/'logs_10.sqlite';stamp=datetime.now(timezone.utc).timestamp()
            with closing(sqlite3.connect(path)) as db, db:
                db.execute('CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT)')
                db.executemany('INSERT INTO logs VALUES(?,?,?,?)',[(index,stamp,'ERROR' if index==1000 else 'INFO','mcp_client') for index in range(1,7001)])
            collector=DiagnosticCollector(home)
            for _ in range(4):collector.refresh()
            self.assertEqual(len(collector.events), 1)
            self.assertFalse(collector.backfill_pending['core'])
            with closing(sqlite3.connect(path)) as db, db:
                db.executemany('INSERT INTO logs VALUES(?,?,?,?)',[(index,stamp,'ERROR' if index in (7001,11000) else 'INFO','mcp_client') for index in range(7001,12001)])
            for _ in range(3):collector.refresh()
            self.assertEqual({row['record_id'] for row in collector.events}, {1000,7001,11000})
            self.assertEqual(collector.sql_cursor, 12000)

    def test_general_logs_and_format_failures_are_visible_without_body(self):
        with tempfile.TemporaryDirectory() as directory:
            collector = DiagnosticCollector(Path(directory))
            collector.desktop_line(f'{TIME} info [future-module] PRIVATE_PROMPT method=refresh conversationId={THREAD}')
            collector.desktop_line(f'{TIME} notice [future-module] PRIVATE_SECRET')
            collector.desktop_line(f'{TIME} critical [future-module] PRIVATE_SECRET')
            collector.desktop_line('unknown format PRIVATE_SECRET')
            self.assertEqual([event['severity'] for event in collector.logs], ['info', 'notice', 'critical'])
            self.assertEqual([event['severity'] for event in collector.events], ['error'])
            self.assertEqual(collector.stats['desktop']['unsupported_lines'], 1)
            self.assertNotIn('PRIVATE', json.dumps(list(collector.logs)))
            self.assertNotIn('PRIVATE', json.dumps(collector.log_sources(True)))

    def test_core_optional_columns_and_all_levels(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            with closing(sqlite3.connect(home/'logs_10.sqlite')) as db, db:
                db.execute('CREATE TABLE logs(id INTEGER PRIMARY KEY,ts INTEGER,level TEXT,target TEXT)')
                db.execute("INSERT INTO logs VALUES(1,1791084778,'DEBUG','future_module')")
            collector = DiagnosticCollector(home);collector.refresh();collector.refresh()
            self.assertEqual(collector.health['core'], 'ok')
            self.assertEqual(len(collector.logs), 1)
            self.assertEqual(collector.logs[0]['severity'], 'debug')
            self.assertEqual(list(collector.events), [])

    def test_logs_are_optional_and_do_not_disable_error_observation(self):
        with tempfile.TemporaryDirectory() as directory:
            dashboard = Dashboard(Path(directory), codex=True)
            dashboard.diagnostics.desktop_line(f'{TIME} info [future-module] PRIVATE')
            self.assertTrue(dashboard.logs()['entries'])
            dashboard.set_settings({'observations': {'logs': False}})
            before = (Path(directory)/'monitoring/local-activity-monitor.jsonl').read_bytes()
            dashboard.monitor.failed(ValueError('PRIVATE'))
            self.assertEqual(dashboard.logs()['entries'], [])
            self.assertEqual((Path(directory)/'monitoring/local-activity-monitor.jsonl').read_bytes(), before)
            self.assertEqual(dashboard.snapshot('all')['errors']['categories']['monitor'], 1)
            dashboard.set_settings({'observations': {'logs': True}})
            self.assertTrue(dashboard.logs()['entries'])
            self.assertNotIn('PRIVATE', json.dumps(dashboard.logs()))

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
