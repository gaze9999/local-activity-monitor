import json
from pathlib import Path
import tempfile
import unittest

from local_activity_monitor.sqlite_records import sqlite_operations, sql_operations, sql_diagnostic
from local_activity_monitor.collectors import CodexCollector


class SQLiteRecordTests(unittest.TestCase):
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
