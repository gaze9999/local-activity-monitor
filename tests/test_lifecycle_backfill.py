from contextlib import closing
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from local_activity_monitor.collectors import CodexCollector

THREAD = "00000000-0000-4000-8000-000000000001"


def record(kind, payload, when="2026-10-04T00:00:00Z"):
    return json.dumps({"type": kind, "payload": payload, "timestamp": when}).encode()+b"\n"


class LifecycleBackfillTests(unittest.TestCase):
    def test_restart_keeps_confirmed_status_and_skill_count_without_recounting_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'sessions';root.mkdir()
            path=root/f'rollout-{THREAD}.jsonl'
            path.write_bytes(record('session_meta',{'id':THREAD})+record('event_msg',{'type':'task_started'})+record('response_item',{'type':'function_call','name':'exec_command','call_id':'skill_read','arguments':json.dumps({'cmd':'Get-Content /skills/example/SKILL.md'})}))
            collector=CodexCollector(root,tail_bytes=8192);collector.refresh()
            self.assertEqual(collector.snapshot()['skills']['counts'],{'example':1})
            with path.open('ab') as stream:
                for _ in range(20):stream.write(record('response_item',{'type':'message','content':'PRIVATE'*100}))
            collector.refresh();collector.thread_state.next_save=0;collector.thread_state.update(collector.files.values())
            restarted=CodexCollector(root,tail_bytes=8192);restarted.refresh()
            value=restarted.snapshot();row=value['threads'][0]
            self.assertEqual(row['status'],'running')
            self.assertEqual(row['status_source'],'cached_lifecycle')
            self.assertFalse(row['status_backfill_pending'])
            self.assertEqual(value['skills']['counts'],{'example':1})
            self.assertNotIn('PRIVATE',json.dumps(collector.thread_state.store.read_document(collector.thread_state.BYTE_LIMIT)))
            with path.open('ab') as stream:stream.write(record('event_msg',{'type':'task_complete'},'2026-10-04T00:02:00Z'))
            restarted.refresh();self.assertEqual(restarted.snapshot()['threads'][0]['status'],'completed')

    def test_checkpoint_gap_does_not_hide_a_newer_offline_completion(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'sessions';root.mkdir();path=root/f'rollout-{THREAD}.jsonl'
            path.write_bytes(record('session_meta',{'id':THREAD})+record('event_msg',{'type':'task_started'}))
            collector=CodexCollector(root,tail_bytes=8192);collector.refresh()
            with path.open('ab') as stream:
                stream.write(record('event_msg',{'type':'task_complete'},'2026-10-04T00:02:00Z'))
                for _ in range(10):stream.write(record('response_item',{'type':'message','content':'PRIVATE'*500}))
            restarted=CodexCollector(root,tail_bytes=8192);restarted.refresh()
            row=restarted.snapshot()['threads'][0]
            self.assertEqual(row['status'],'completed')
            self.assertEqual(row['status_source'],'session_lifecycle')

    def test_long_turn_recovers_status_with_shared_read_budget(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root/f"rollout-{THREAD}.jsonl"
            with path.open("wb") as stream:
                stream.write(record("session_meta", {"id": THREAD}))
                stream.write(record("event_msg", {"type": "task_complete", "started_at": "2026-10-03T23:00:00Z"}))
                stream.write(record("event_msg", {"type": "task_started"}, "2026-10-04T00:01:00Z"))
                padding = record("response_item", {"type": "message", "content": "PRIVATE"*3000})
                for _ in range(500):
                    stream.write(padding)
                stream.write(record("token_usage_record", {"thread_token_usage": {"total_tokens": 123}}, "2026-10-04T00:02:00Z"))
            collector = CodexCollector(root, tail_bytes=8192);collector.refresh()
            self.assertLessEqual(collector.read_bytes, 8*1024*1024)
            self.assertTrue(collector.snapshot()["threads"][0]["status_backfill_pending"])
            for _ in range(20):
                before = collector.read_bytes;collector.refresh()
                self.assertLessEqual(collector.read_bytes-before, 8*1024*1024)
                if not collector.snapshot()["threads"][0]["status_backfill_pending"]:
                    break
            row = collector.snapshot()["threads"][0]
            self.assertEqual(row["status"], "running")
            self.assertEqual(row["status_source"], "session_lifecycle")
            self.assertFalse(row["status_backfill_pending"])
            self.assertEqual(row["tokens"]["total_tokens"], 123)
            self.assertNotIn("PRIVATE", json.dumps(row))
            for _ in range(50):
                if not any(state.get("error_cursor") for state in collector.files.values()):break
                before = collector.read_bytes;collector.refresh()
                self.assertLessEqual(collector.read_bytes-before, 8*1024*1024)
            self.assertFalse(any(state.get("error_cursor") for state in collector.files.values()))
            before = collector.read_bytes;collector.refresh();self.assertEqual(collector.read_bytes, before)

    def test_complete_event_can_supply_duration_without_start_in_tail(self):
        collector = CodexCollector(Path("unused"))
        collector.features["metadata"] = False
        state = collector.new_file(Path(f"rollout-{THREAD}.jsonl"))
        collector.files[Path("unused")] = state
        collector.consume(state, {"type": "event_msg", "timestamp": "2026-10-04T01:02:00Z", "payload": {"type": "task_complete", "started_at": "2026-10-04T01:00:00Z", "completed_at": "2026-10-04T01:02:00Z"}})
        collector.consume(state, {"type": "event_msg", "timestamp": "2026-10-04T00:00:00Z", "payload": {"type": "task_started"}})
        row = collector.snapshot()["threads"][0]
        self.assertEqual(row["status"], "completed")
        self.assertEqual(row["task_duration_ms"], 120000)

    def test_cloud_catalog_optional_fields_and_missing_counters(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory);(home/"sqlite").mkdir();(home/"sessions").mkdir()
            with closing(sqlite3.connect(home/"sqlite/codex-dev.db")) as db, db:
                db.execute("CREATE TABLE local_thread_catalog(host_id TEXT,thread_id TEXT,display_title TEXT,source_created_at REAL,source_updated_at REAL,source_kind TEXT,thread_source TEXT,project_id TEXT,model TEXT,reasoning_effort TEXT,model_provider TEXT,chatgpt_async_status TEXT)")
                db.execute("INSERT INTO local_thread_catalog VALUES(?,?,?,?,?,?,?,?,?,?,?,?)", ("chatgpt:fixture", THREAD, "雲端對話", 1791072000, 1791072060, "chatgpt", None, None, "future-model", "future-effort", "openai", "future_status"))
            collector = CodexCollector(home/"sessions");collector.refresh()
            row = collector.snapshot()["threads"][0]
            self.assertTrue(row["metadata_only"])
            self.assertEqual(row["status_source"], "catalog_status")
            self.assertEqual(row["status"], "future_status")
            self.assertEqual(row["model"], "future-model")
            self.assertEqual(row["reasoning_effort"], "future-effort")
            self.assertEqual(row["execution"]["model_provider"], "openai")
            self.assertIsNone(row["tool_calls"])
            self.assertEqual(row["tokens"], {})


if __name__ == "__main__":
    unittest.main()
