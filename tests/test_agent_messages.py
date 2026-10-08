import json
import unittest

from local_activity_monitor.agent_messages import agent_messages, incoming_message, parse_message_detail


class AgentMessageTests(unittest.TestCase):
    def test_direct_tool_names_and_namespace_preserve_metadata_without_body(self):
        for name, namespace in [("collaboration.send_message", None), ("send_message", "collaboration"), ("functions.send_message", None), ("functions.collaboration.send_message", None)]:
            with self.subTest(name=name):
                payload = {"name": name, "namespace": namespace, "arguments": json.dumps({"target": "/root/worker", "message": "PRIVATE BODY", "api_key": "SECRET"})}
                rows = agent_messages(payload, {"agent_name": "/root"})
                self.assertEqual(len(rows), 1)
                self.assertEqual(rows[0]["target"], "/root/worker")
                self.assertEqual(rows[0]["sender"], "/root")
                self.assertEqual(rows[0]["direction"], "outgoing")
                self.assertFalse(rows[0]["nested"])
                self.assertNotIn("PRIVATE", json.dumps(rows))
                self.assertNotIn("SECRET", json.dumps(rows))

    def test_nested_literal_calls_have_stable_matching_indexes_without_execution(self):
        payload = {"name": "functions.exec", "arguments": 'await tools.spawn_agent({task_name:"worker",message:"PRIVATE"}); await tools.exec_command({cmd:"PRIVATE"}); await tools.collaboration__send_message({target:"worker",message:"PRIVATE"}); await tools.followup_task(dynamicArgs);'}
        rows = agent_messages(payload)
        self.assertEqual([row["action"] for row in rows], ["spawn_agent", "send_message", "followup_task"])
        self.assertEqual([row["index"] for row in rows], [0, 1, 2])
        self.assertEqual([row["invocation_index"] for row in rows], [0, 2, 3])
        self.assertEqual(rows[0]["task_name"], "worker")
        self.assertIsNone(rows[0]["target"])
        self.assertIsNone(rows[2]["target"])
        self.assertTrue(all(row["nested"] for row in rows))
        self.assertNotIn("PRIVATE", json.dumps(rows))

    def test_invalid_unrelated_and_unknown_fields_remain_absent_or_null(self):
        self.assertEqual(agent_messages(None), [])
        self.assertEqual(agent_messages({"name": "unrelated.send_message", "arguments": {}}), [])
        self.assertEqual(agent_messages({"name": "functions.exec", "arguments": 'const example="tools.send_message({target:PRIVATE})"; // tools.send_message({target:"PRIVATE"})'}), [])
        rows = agent_messages({"name": "followup_task", "arguments": {"target": "PRIVATE\nbody", "message": "PRIVATE"}}, {"agent_name": "invalid space"})
        self.assertIsNone(rows[0]["target"])
        self.assertIsNone(rows[0]["sender"])
        self.assertNotIn("PRIVATE", json.dumps(rows))

    def test_incoming_envelope_does_not_project_payload_or_claim_verified_delivery(self):
        for kind in ("MESSAGE", "NEW_TASK", "FINAL_ANSWER"):
            row = incoming_message(f"Message Type: {kind}\nTask name: /root/worker\nSender: /root\nPayload:\nPRIVATE MESSAGE BODY")
            self.assertEqual(row["direction"], "incoming")
            self.assertEqual(row["sender"], "/root")
            self.assertEqual(row["target"], "/root/worker")
            self.assertEqual(row["recognition"], "recorded_message_header")
            self.assertNotIn("PRIVATE", json.dumps(row))
        self.assertIsNone(incoming_message("example\nMessage Type: MESSAGE\nTask name: /root\nSender: /root/worker\nPayload:\nPRIVATE"))
        self.assertIsNone(incoming_message("Message Type: MESSAGE\nPRIVATE"))

    def test_detail_is_only_on_demand_and_masks_recorded_context_by_default(self):
        payload = {"name": "collaboration.spawn_agent", "arguments": {"task_name": "worker", "message": "api_key=SECRET", "fork_turns": "all", "context": {"text": "PRIVATE CONTEXT", "authorization": "SECRET"}}}
        rows = agent_messages(payload)
        self.assertNotIn("PRIVATE", json.dumps(rows))
        masked = parse_message_detail(payload)
        self.assertEqual(masked["request"]["message"], "[已隱藏]")
        self.assertEqual(masked["request"]["context"]["authorization"], "[已隱藏]")
        self.assertEqual(masked["request"]["context"]["text"], "PRIVATE CONTEXT")
        self.assertEqual(masked["context_state"], "recorded")
        unmasked = parse_message_detail(payload, mask=False)
        self.assertEqual(unmasked["request"], payload["arguments"])
        payload["arguments"].pop("context")
        self.assertEqual(parse_message_detail(payload)["context_state"], "not_recorded")
        self.assertEqual(parse_message_detail(payload)["request"]["fork_turns"], "all")

    def test_detail_selects_only_matching_descriptor_and_rejects_invalid_index(self):
        payload = {"name": "functions.exec", "arguments": 'await tools.exec_command({cmd:"not a message"}); await tools.send_message({target:"worker",message:"FIRST"}); await tools.followup_task({target:"worker",message:"SECOND"});'}
        self.assertEqual(parse_message_detail(payload, 1)["request"]["message"], "SECOND")
        self.assertIsNone(parse_message_detail(payload, 2))
        self.assertIsNone(parse_message_detail(payload, True))
        self.assertIsNone(parse_message_detail(payload, -1))
        self.assertIsNone(parse_message_detail(payload, mask="false"))
        unresolved = parse_message_detail({"name": "functions.exec", "arguments": 'await tools.send_message(dynamicArgs);'})
        self.assertIsNone(unresolved["request"])
        self.assertEqual(unresolved["context_state"], "not_recorded")


if __name__ == "__main__":
    unittest.main()
