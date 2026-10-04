import unittest
from pathlib import Path
import tempfile
from unittest.mock import patch

from local_activity_monitor.mcp_records import response_metadata, summarize
from local_activity_monitor.server import Dashboard, configure


class RecordingStatusTests(unittest.TestCase):
    def event(self, result, when="2026-10-05T00:00:00Z", server="future"):
        return {"server": server, "tool": "status", "timestamp": when,
                "completed_at": when, "nested": False, "result": result}

    def status(self, events):
        return summarize({"future": "configured"}, events)["recording_status"]

    def test_unknown_and_non_boolean_values_remain_unknown(self):
        for result in ({}, {"recording_enabled": "false"}, {"telemetry_enabled": 0},
                       {"recording_enabled": None}, {"unrelated_enabled": True}):
            with self.subTest(result=result):
                self.assertEqual(self.status([self.event(result)]), {})

    def test_false_and_original_field_evidence_are_preserved(self):
        result = response_metadata("future", {"recording_enabled": False, "status": "ok"})
        state = self.status([self.event(result)])["future"]
        self.assertFalse(state["enabled"])
        self.assertEqual(state["key"], "recording_enabled")
        self.assertEqual(state["flags"]["recording_enabled"], {
            "enabled": False, "health": "ok", "observed_at": "2026-10-05T00:00:00Z"})

    def test_mixed_fields_ignore_dictionary_order(self):
        pairs = [("telemetry_enabled", True), ("recording_enabled", False)]
        first = self.status([self.event(dict(pairs))])["future"]
        second = self.status([self.event(dict(reversed(pairs)))])["future"]
        self.assertEqual(first, second)
        self.assertFalse(first["enabled"])
        self.assertTrue(first["flags"]["telemetry_enabled"]["enabled"])

    def test_telemetry_only_and_prefixed_fields_are_generic(self):
        for field in ("telemetry_enabled", "metrics_recording_enabled", "usage_telemetry_enabled"):
            with self.subTest(field=field):
                state = self.status([self.event({field: False})])["future"]
                self.assertEqual(state["key"], field)
                self.assertFalse(state["flags"][field]["enabled"])

    def test_latest_report_updates_only_its_fields(self):
        old = self.event({"recording_enabled": False, "telemetry_enabled": False, "status": "error"})
        new = self.event({"telemetry_enabled": True, "status": "ok"}, "2026-10-05T00:01:00Z")
        state = self.status([new, old])["future"]
        self.assertTrue(state["enabled"])
        self.assertEqual(state["key"], "telemetry_enabled")
        self.assertEqual(state["observed_at"], new["completed_at"])
        self.assertEqual(state["health"], "ok")
        self.assertFalse(state["flags"]["recording_enabled"]["enabled"])
        self.assertEqual(state["flags"]["recording_enabled"]["observed_at"], old["completed_at"])
        newest = self.event({"recording_enabled": True}, "2026-10-05T00:02:00Z")
        state = self.status([newest, old, new])["future"]
        self.assertTrue(state["flags"]["recording_enabled"]["enabled"])
        self.assertTrue(state["flags"]["telemetry_enabled"]["enabled"])
        self.assertIsNone(state["health"])

    def test_projection_needs_no_file_or_network_io(self):
        with patch("pathlib.Path.open", side_effect=AssertionError("unexpected file read")), \
             patch("socket.socket", side_effect=AssertionError("unexpected network call")):
            state = self.status([self.event({"recording_enabled": True}, server="unknown-new-source")])
        self.assertTrue(state["unknown-new-source"]["enabled"])

    def test_non_boolean_newer_report_does_not_erase_evidence(self):
        old = self.event({"recording_enabled": False})
        new = self.event({"recording_enabled": "true"}, "2026-10-05T00:01:00Z")
        self.assertEqual(self.status([new, old]), self.status([old]))

    def test_completed_time_and_same_kind_priority(self):
        old = self.event({"recording_enabled": False}, "2026-10-05T00:01:00Z")
        new = self.event({"z_recording_enabled": False, "a_recording_enabled": True})
        new["completed_at"] = "2026-10-05T00:02:00Z"
        state = self.status([new, old])["future"]
        self.assertTrue(state["enabled"])
        self.assertEqual(state["key"], "a_recording_enabled")
        self.assertEqual(state["observed_at"], new["completed_at"])

    def test_jev_config_does_not_erase_observed_flags_or_assume_disabled(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            dashboard = Dashboard(home, codex=True)
            event = self.event({'recording_enabled': True, 'status': 'ok'}, server='jev')
            with patch.object(dashboard.codex, 'snapshot', return_value={'health': 'ok', 'threads': [], 'tools': {}, 'mcp_events': [event]}):
                dashboard.refresh()
                state = dashboard.snapshot('all')['mcp']['recording_status']['jev']
                self.assertTrue(state['enabled'])
                self.assertEqual(state['health'], 'ok')
                self.assertEqual(state['observed_at'], event['completed_at'])
                self.assertNotIn('config.recording_enabled', state['flags'])
                configure(home, False, home/'unused.sqlite')
                dashboard.refresh()
                state = dashboard.snapshot('all')['mcp']['recording_status']['jev']
                self.assertTrue(state['enabled'])
                self.assertTrue(state['flags']['recording_enabled']['enabled'])
                self.assertFalse(state['flags']['config.recording_enabled']['enabled'])
                self.assertEqual(state['flags']['config.recording_enabled']['source'], 'monitor_config')
                self.assertEqual(state['collector_health'], 'disabled')
                (home/'monitoring/jev-monitor.json').write_text('invalid', encoding='utf-8')
                dashboard.refresh()
                state = dashboard.snapshot('all')['mcp']['recording_status']['jev']
                self.assertTrue(state['enabled'])
                self.assertNotIn('config.recording_enabled', state['flags'])
                self.assertEqual(state['collector_health'], 'invalid_config')


if __name__ == "__main__":
    unittest.main()
