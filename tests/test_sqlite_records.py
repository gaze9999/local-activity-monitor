import json
from datetime import datetime, timezone
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
import warnings
from unittest.mock import MagicMock

from local_activity_monitor.sqlite_records import diagnostic_content, sql_content, sqlite_operations, sql_operations, sql_diagnostic
from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.server import Dashboard, handler
from local_activity_monitor.error_records import DiagnosticCollector


class SQLiteRecordTests(unittest.TestCase):
    def test_forward_sql_log_backfill_keeps_pending_until_latest_row(self):
        with tempfile.TemporaryDirectory() as folder:
            home=Path(folder)
            path=home/'logs_1.sqlite'
            stamp=datetime.now(timezone.utc).timestamp()
            with closing(sqlite3.connect(path)) as db,db:
                db.execute('CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT, feedback_log_body TEXT)')
                db.execute("INSERT INTO logs VALUES(1,?,'INFO','sqlx::query','sql=\"SELECT 1\" elapsed=1ms')",(stamp,))
            collector=DiagnosticCollector(home)
            collector.refresh_core()
            with closing(sqlite3.connect(path)) as db,db:
                db.executemany("INSERT INTO logs VALUES(?,?,'INFO','sqlx::query','sql=\"SELECT 1\" elapsed=1ms')",[(index,stamp) for index in range(2,4502)])
            for expected,pending in ((2001,True),(4001,True),(4501,False)):
                collector.refresh_core()
                self.assertEqual(collector.sql_cursor,expected)
                self.assertEqual(collector.backfill_pending['core'],pending)
            self.assertEqual(collector.sql_events[-1]['record_id'],4501)

    def test_recorded_escape_warning_does_not_escape_metadata_parser(self):
        code = 'pattern = "' + chr(92) + '["\nimport sqlite3\nc=sqlite3.connect(":memory:")\nc.execute("SELECT 1")'
        with warnings.catch_warnings(record=True) as emitted:
            warnings.simplefilter("always")
            rows = sqlite_operations([("exec_command", {"cmd": code}, False)], True)
        self.assertEqual(emitted, [])
        self.assertEqual(rows[-1]["sql"], "SELECT 1")
    def test_selected_statements_keep_literals_comments_and_multiline_sql(self):
        sql = "-- statement\nSELECT 'semi;colon', 'comma,value';\nUPDATE items SET value='it''s valid'"
        rows = sqlite_operations([("mcp__sqlite__query", {"sql": sql}, False)], include_sql=True)
        self.assertEqual(rows[0]["sql"], "-- statement\nSELECT 'semi;colon', 'comma,value'")
        self.assertEqual(rows[1]["sql"], "UPDATE items SET value='it''s valid'")
        self.assertNotIn("sql", sqlite_operations([("mcp__sqlite__query", {"sql": sql}, False)])[0])
        code = "import sqlite3\nc=sqlite3.connect(':memory:')\nc.execute('SELECT 42')\nc.execute(dynamic_sql)"
        self.assertEqual([row.get("sql") for row in sqlite_operations([("exec_command", {"cmd": code}, False)], True)], [None, "SELECT 42"])

    def test_sql_credentials_and_display_limit(self):
        sql = "INSERT INTO accounts (name, password, api_key) VALUES ('visible,one', 'PRIVATE(x)', 'PRIVATE2'),('it''s fine','PRIVATE3','PRIVATE4'); UPDATE accounts SET password='PRIVATE5'"
        result = sql_content(sql)
        self.assertNotIn("PRIVATE", result["sql"])
        self.assertIn("visible,one", result["sql"])
        self.assertIn("it''s fine", result["sql"])
        self.assertFalse(result["truncated"])
        result = sql_content("SELECT '"+"x"*40000+"'")
        self.assertTrue(result["truncated"])
        self.assertEqual(len(result["sql"]), 32768)
        self.assertIsNone(diagnostic_content("codex_core", 'sql="SELECT PRIVATE"')["sql"])
        self.assertIsNone(diagnostic_content("sqlx::query", 'elapsed=1ms')["sql"])
        self.assertEqual(diagnostic_content("sqlx::query", 'sql="SELECT 1\\nFROM items" elapsed=1ms')["sql"], "SELECT 1\nFROM items")

    def test_sql_detail_endpoint_reads_only_known_records_and_respects_switch(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder);sessions=home/"sessions";sessions.mkdir()
            thread="00000000-0000-4000-8000-000000000001"
            path=sessions/("rollout-"+thread+".jsonl")
            stamp=datetime.now(timezone.utc).isoformat()
            records=[{"timestamp":stamp, "type":"session_meta", "payload":{"id":thread}}, {"timestamp":stamp, "type":"response_item", "payload":{"type":"function_call", "name":"exec_command", "call_id":"sql-call", "arguments":json.dumps({"cmd":'sqlite3 demo.db "SELECT 12; SELECT 34"'})}}]
            records.append({"timestamp":stamp, "type":"response_item", "payload":{"type":"function_call_output", "call_id":"sql-call", "output":json.dumps({"rows":[12,34],"api_key":"SQL_PRIVATE_RESULT"})}})
            records.append({"timestamp":stamp, "type":"response_item", "payload":{"type":"function_call_output", "call_id":"other-call", "output":"UNRELATED_RESULT"}})
            path.write_text("\n".join(json.dumps(row) for row in records)+"\n",encoding="utf-8")
            dashboard=Dashboard(home,codex=True);dashboard.refresh()
            events=dashboard.cache["24h"]["codex"]["sqlite"]["events"]
            select=next(event for event in events if event["index"]==2)
            self.assertNotIn("SELECT 34",json.dumps(dashboard.cache))
            self.assertEqual(dashboard.sql_detail(select["id"])["sql"],"SELECT 34")
            for full in (False, True):
                detail = dashboard.sql_detail(select["id"], full=full)
                self.assertEqual(detail["response"]["rows"], [12,34])
                self.assertEqual(detail["response_scope"], "containing_tool_call")
                self.assertNotIn("SQL_PRIVATE_RESULT", json.dumps(detail))
                self.assertNotIn("UNRELATED_RESULT", json.dumps(detail))
            self.assertNotIn("SQL_PRIVATE_RESULT", json.dumps(dashboard.cache))
            instance=object.__new__(handler(dashboard,8787))
            for path,host,expected in [("/api/codex/sql?id="+select["id"],"127.0.0.1:8787",200),("/api/codex/sql?id="+"f"*64,"127.0.0.1:8787",409),("/api/codex/sql?id=../../auth.json","127.0.0.1:8787",400),("/api/codex/sql?id="+select["id"]+"&file=auth.json","127.0.0.1:8787",400),("/api/codex/sql?id="+select["id"],"external.example:8787",403)]:
                instance.path=path;instance.headers={"Host":host};instance.reply=MagicMock();instance.do_GET()
                self.assertEqual(instance.reply.call_args.args[0],expected)
            dashboard.observations["sqlite"]=False
            self.assertIsNone(dashboard.sql_detail(select["id"]))

    def test_diagnostic_detail_uses_exact_core_row_and_desktop_offset(self):
        with tempfile.TemporaryDirectory() as folder:
            home=Path(folder)
            with closing(sqlite3.connect(home/"logs_1.sqlite")) as db,db:
                db.execute("CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT, feedback_log_body TEXT)")
                db.execute("INSERT INTO logs VALUES(1,?,'INFO','sqlx::query',?)", (datetime.now(timezone.utc).timestamp(), 'sql="SELECT 45" elapsed=1ms'))
            root=home/"logs";root.mkdir();path=root/"codex-desktop-test.log"
            stamp=datetime.now(timezone.utc).isoformat()
            lines=[stamp+' INFO [sqlite] sql="SELECT 67" elapsed=2ms', stamp+' INFO [sqlite] sql="SELECT 89" elapsed=3ms']
            path.write_text("\n".join(lines)+"\n",encoding="utf-8")
            dashboard=Dashboard(home,codex=True);dashboard.diagnostics.roots=[root];dashboard.refresh()
            events=dashboard.cache["24h"]["codex"]["sqlite"]["events"]
            self.assertEqual({dashboard.sql_detail(event["id"])["sql"] for event in events}, {"SELECT 45","SELECT 67","SELECT 89"})
            self.assertNotIn("SELECT 45",json.dumps(dashboard.cache))
            original=next(event for event in events if event.get("source")=="codex_desktop")
            path.write_text("changed line\n",encoding="utf-8")
            self.assertIsNone(dashboard.sql_detail(original["id"])["sql"])

    def test_sql_diagnostic_requires_module_and_projects_only_measured_fields(self):
        body = 'summary="SELECT PRIVATE FROM secret" elapsed=241µs rows_affected=0 rows_returned=3 password="PRIVATE elapsed=900ms"'
        value = sql_diagnostic("sqlx::query", body)
        self.assertEqual((value["statement"], value["operation"], value["duration_ms"]), ("SELECT", "read", .241))
        self.assertEqual((value["rows_affected"], value["rows_returned"]), (0, 3))
        self.assertIsNone(value["database"])
        self.assertNotIn("PRIVATE", json.dumps(value))
        self.assertIsNone(sql_diagnostic("codex_core", body))
        value = sql_diagnostic("codex_state::state_db", 'unknown PRIVATE elapsed_secs=0.00125 rows_returned=9999999999999999999')
        self.assertEqual(value["duration_ms"], 1.25)
        self.assertIsNone(value["rows_returned"])
        self.assertEqual(value["statement"], "DATABASE_EVENT")
        self.assertEqual(value["engine"], "SQLite")
        self.assertIsNone(sql_diagnostic("sqlite", 'duration=999999999999999999999s')["duration_ms"])

    def test_literals_cte_and_private_values(self):
        sql = "WITH a AS (SELECT 'PRIVATE') UPDATE items SET value='PRIVATE; DROP TABLE items'; SELECT 1; -- DELETE\nPRAGMA query_only=ON"
        self.assertEqual(sql_operations(sql), [("UPDATE", "write"), ("SELECT", "read"), ("PRAGMA", "pragma")])
        self.assertEqual(sql_operations("documentation contains SELECT"), [])
        result = sqlite_operations([("exec_command", {"cmd": 'sqlite3 "demo.db" "'+sql.replace('"', '')+'"'}, False)])
        self.assertEqual(result[0]["database"], "demo.db")
        self.assertNotIn("PRIVATE", json.dumps(result))

    def test_python_aliases_cursor_and_here_string(self):
        code = "import sqlite3 as sql\nconn=sql.connect('demo.db')\ncursor=conn.cursor()\ncursor.execute('SELECT PRIVATE')\nconn.commit()\nconn.close()"
        result = sqlite_operations([("exec_command", {"cmd": "@'\n"+code+"\n'@ | python -", "workdir": "C:/demo"}, True)])
        self.assertEqual([row["operation"] for row in result], ["open", "read", "transaction", "close"])
        self.assertEqual({row["database"] for row in result}, {"demo.db"})
        self.assertNotIn("PRIVATE", json.dumps(result))

    def test_mcp_unknown_operation_dynamic_database_and_cap(self):
        result = sqlite_operations([("mcp__sqlite__query", {"query": "SELECT 'PRIVATE'", "database": "file:demo.db?password=PRIVATE"}, False)])
        self.assertEqual(result[0]["database"], "file:demo.db")
        self.assertEqual(result[0]["recognition"], "mcp_call")
        self.assertNotIn("PRIVATE", json.dumps(result))
        result = sqlite_operations([("exec_command", {"cmd": "import sqlite3\nc=sqlite3.connect(path)\nc.execute('SELECT 1')"}, False)])
        self.assertIsNone(result[0]["database"])
        self.assertLessEqual(len(sql_operations("SELECT 1;"*1000)), 80)

    def test_collector_result_is_outer_tool_not_sql_duration(self):
        with tempfile.TemporaryDirectory() as folder:
            collector = CodexCollector(Path(folder))
            state = collector.new_file(Path(folder)/"rollout-00000000-0000-4000-8000-000000000001.jsonl");collector.files[Path(folder)/"fixture"] = state
            collector.consume(state, {"timestamp": "2026-10-04T00:00:00Z", "type": "response_item", "payload": {"type": "function_call", "name": "exec_command", "call_id": "call_1", "arguments": json.dumps({"cmd": 'sqlite3 demo.db "SELECT 1"'})}})
            collector.consume(state, {"timestamp": "2026-10-04T00:00:02Z", "type": "response_item", "payload": {"type": "function_call_output", "call_id": "call_1", "output": {"exit_code": 1, "message": "PRIVATE"}}})
            events = collector.snapshot()["sqlite"]["events"]
            self.assertEqual(events[0]["result"], "failed")
            self.assertIsNone(events[0]["duration_ms"])
            self.assertEqual(events[0]["container_duration_ms"], 2000)
            self.assertNotIn("PRIVATE", json.dumps(events))
            collector.features["sqlite"] = False
            self.assertEqual(collector.snapshot()["sqlite"]["events"], [])


if __name__ == "__main__":
    unittest.main()
