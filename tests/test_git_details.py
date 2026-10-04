import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch
from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.server import Dashboard, handler

THREAD = "00000000-0000-0000-0000-000000000001"

class GitDetailTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
        self.root = self.home/"sessions"
        self.root.mkdir()
        self.path = self.root/f"rollout-{THREAD}.jsonl"

    def tearDown(self):
        self.temp.cleanup()

    def call(self, command, identity="one", nested=False, output="reply"):
        payload = {"type":"function_call", "name":"functions.exec" if nested else "exec_command", "call_id":identity, "arguments":f"text(await tools.exec_command({{cmd:{json.dumps(command)}}}));" if nested else json.dumps({"cmd":command})}
        records = [{"type":"response_item", "timestamp":"2026-10-05T00:00:00Z", "payload":payload}, {"type":"response_item", "timestamp":"2026-10-05T00:00:01Z", "payload":{"type":"function_call_output", "call_id":identity, "output":output}}]
        with self.path.open("a", encoding="utf-8") as stream:
            for record in records:
                stream.write(json.dumps(record)+"\n")

    def test_exact_event_and_no_snapshot_content_or_unobserved_reads(self):
        self.call("git status", output="PRIVATE_OUTPUT")
        collector = CodexCollector(self.root)
        collector.refresh()
        result = collector.git_detail(THREAD,"one","status")
        self.assertEqual(result["commands"], ["git status"])
        self.assertEqual(result["output"], "PRIVATE_OUTPUT")
        self.assertEqual(result["output_scope"], "git_command")
        self.assertNotIn("PRIVATE_OUTPUT", json.dumps(collector.snapshot()))
        with patch.object(Path,"open",side_effect=AssertionError("unexpected read")):
            self.assertEqual(collector.git_detail(THREAD,"unknown","status")["commands"], [])
            self.assertEqual(collector.git_detail(THREAD,"one","diff")["commands"], [])
            collector.features["metadata"] = False
            collector.snapshot()
        collector.tail_bytes = 1
        self.assertEqual(collector.git_detail(THREAD,"one","status")["commands"], [])

    def test_mixed_shell_nested_output_scope_and_redaction(self):
        self.call("git status; git diff; echo extra", output="password=SECRET")
        collector = CodexCollector(self.root);collector.refresh()
        result = collector.git_detail(THREAD,"one","diff")
        self.assertEqual(result["commands"], ["git diff"])
        self.assertEqual(result["output_scope"], "outer_scope")
        self.assertNotIn("SECRET",result["output"])
        self.call("git -c token=SECRET status", "two", nested=True, output="x"*40000)
        collector.refresh()
        result = collector.git_detail(THREAD,"two","status")
        self.assertEqual(result["output_scope"], "outer_scope")
        self.assertNotIn("SECRET",json.dumps(result))
        self.assertLessEqual(len(result["output"]),32768)
        self.assertTrue(result["truncated"])

    def test_missing_and_symlink_session_do_not_follow_replacement(self):
        self.call("git status")
        collector = CodexCollector(self.root);collector.refresh()
        with patch.object(Path, "is_symlink", return_value=True), patch.object(Path, "open", side_effect=AssertionError("symlink read")):
            self.assertEqual(collector.git_detail(THREAD,"one","status")["commands"], [])
        self.path.unlink()
        self.assertEqual(collector.git_detail(THREAD,"one","status")["commands"], [])

    def test_http_validation_and_disabled(self):
        self.call("git status")
        dashboard = Dashboard(self.home,codex=True);dashboard.refresh()
        instance = object.__new__(handler(dashboard,8787));instance.headers={"Host":"127.0.0.1:8787"};instance.reply=MagicMock()
        valid = f"thread_id={THREAD}&call_id=one&operation=status"
        for query in ("",valid+"&extra=x",valid+"&call_id=two",valid.replace("one","../one"),valid.replace("status","status%0A")):
            instance.path="/api/codex/git?"+query;instance.do_GET()
            self.assertEqual(instance.reply.call_args.args[0],400)
        instance.path="/api/codex/git?"+valid;instance.do_GET()
        self.assertEqual(instance.reply.call_args.args[0],200)
        dashboard.set_settings({"observations":{"git":False}});instance.do_GET()
        self.assertEqual(instance.reply.call_args.args[0],409)
