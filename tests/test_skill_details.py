import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.server import Dashboard, handler
from local_activity_monitor.thread_state import ThreadState

THREAD = "00000000-0000-0000-0000-000000000001"


class SkillDetailTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
        self.root = self.home/"sessions"
        self.root.mkdir()
        self.path = self.root/f"rollout-{THREAD}.jsonl"

    def tearDown(self):
        self.temp.cleanup()

    def add(self, origin, call):
        directory = self.home/origin/"example"
        directory.mkdir(parents=True)
        document = directory/"SKILL.md"
        document.write_text(origin, encoding="utf-8")
        payload = {"type":"function_call", "name":"exec_command", "call_id":call, "arguments":json.dumps({"cmd":f'Get-Content "{document}"'})}
        with self.path.open("a", encoding="utf-8") as stream:
            stream.write(json.dumps({"type":"response_item", "timestamp":"2026-10-05T00:00:00Z", "payload":payload})+"\n")
        return document

    def test_same_name_is_scoped_to_event_and_name_only_is_ambiguous(self):
        self.add("first", "one")
        self.add("second", "two")
        collector = CodexCollector(self.root)
        collector.refresh()
        self.assertEqual(collector.skill_detail("example", thread_id=THREAD, call_id="one")["documents"][0]["text"], "first")
        self.assertEqual(collector.skill_detail("example", thread_id=THREAD, call_id="two")["documents"][0]["text"], "second")
        self.assertEqual(collector.skill_detail("example")["documents"], [])
        self.assertEqual(collector.skill_detail("example", thread_id=THREAD, call_id="unknown")["documents"], [])
        self.assertNotIn(str(self.home), json.dumps(collector.snapshot()["skills"]))

    def test_restart_checkpoint_restores_only_bounded_observed_invocation(self):
        document = self.add("first", "one")
        collector = CodexCollector(self.root)
        collector.refresh()
        collector.thread_state.update(list(collector.files.values()))
        checkpoint = collector.thread_state.path
        self.assertNotIn("doc_path", checkpoint.read_text(encoding="utf-8"))
        restored = CodexCollector(self.root)
        restored.refresh()
        restored.thread_state = ThreadState(checkpoint)
        for state in restored.files.values():
            state["calls"].clear()
        result = restored.skill_detail("example", thread_id=THREAD, call_id="one")
        self.assertEqual(result["documents"][0]["text"], "first")
        restored.tail_bytes = 1
        self.assertEqual(restored.skill_detail("example", thread_id=THREAD, call_id="one")["documents"], [])
        restored.tail_bytes = 1024*1024
        document.unlink()
        self.assertEqual(restored.skill_detail("example", thread_id=THREAD, call_id="one")["documents"], [])
        restored.files.clear()
        self.assertEqual(restored.skill_detail("example", thread_id=THREAD, call_id="one")["documents"], [])

    def test_unobserved_and_unc_do_not_read_documents(self):
        self.add("first", "one")
        collector = CodexCollector(self.root)
        collector.refresh()
        with patch.object(Path, "open", side_effect=AssertionError("unobserved read")):
            self.assertEqual(collector.skill_detail("example", thread_id=THREAD, call_id="unknown")["documents"], [])
        for state in collector.files.values():
            state["calls"]["one"]["skills"][0]["doc_path"] = "//remote/share/example/SKILL.md"
        with patch.object(Path, "open", side_effect=AssertionError("UNC read")):
            self.assertEqual(collector.skill_detail("example", thread_id=THREAD, call_id="one")["documents"], [])

    def test_name_only_checkpoint_lookup_has_shared_read_budget(self):
        self.add("first", "one")
        collector = CodexCollector(self.root);collector.refresh()
        collector.thread_state.update(list(collector.files.values()))
        original_path, original_state = next(iter(collector.files.items()))
        original_state["calls"].clear()
        for index in range(10):
            target = self.root/f"copy-{index}.jsonl"
            target.write_bytes(original_path.read_bytes())
            collector.files[target] = dict(original_state)
        collector.READ_LIMIT = 20
        collector.tail_bytes = 10
        opened = []
        original = Path.open
        def counted(path, *args, **kwargs):
            if path.suffix == ".jsonl":
                opened.append(path)
            return original(path, *args, **kwargs)
        with patch.object(Path, "open", counted):
            self.assertEqual(collector.skill_detail("example")["documents"], [])
        self.assertEqual(len(opened), 2)

    def test_http_event_validation_and_disabled_observation(self):
        self.add("first", "one")
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        instance = object.__new__(handler(dashboard, 8787))
        instance.headers = {"Host":"127.0.0.1:8787"}
        instance.reply = MagicMock()
        for query in ("skill=example&thread_id=bad&call_id=one", f"skill=example&thread_id={THREAD}", f"skill=example&thread_id={THREAD}&call_id=../one", f"skill=example&thread_id={THREAD}&call_id=one&file=../outside.md"):
            instance.path = "/api/codex/skill?"+query
            instance.do_GET()
            self.assertEqual(instance.reply.call_args.args[0], 400)
        instance.path = f"/api/codex/skill?skill=example&thread_id={THREAD}&call_id=one"
        instance.do_GET()
        self.assertEqual(instance.reply.call_args.args[0], 200)
        dashboard.set_settings({"observations":{"skills":False}})
        instance.do_GET()
        self.assertEqual(instance.reply.call_args.args[0], 409)
