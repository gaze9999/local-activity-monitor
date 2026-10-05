from datetime import datetime, timezone
import unittest

from local_activity_monitor.codex_connection import connection_status


class CodexConnectionTests(unittest.TestCase):
    current = datetime(2026, 10, 5, 1, 10, tzinfo=timezone.utc)

    def status(self, threads=(), errors=(), enabled=True):
        return connection_status(threads, errors, enabled, self.current)

    def test_recent_response_does_not_depend_on_collector_health_or_tool_activity(self):
        self.assertEqual(self.status([{"status": "running", "updated_at": "2026-10-05T01:09:00Z", "health": "ok"}])["state"], "unknown")
        result = self.status([{"token_updated_at": "2026-10-05T01:09:00Z"}])
        self.assertEqual(result["state"], "response")
        self.assertEqual(result["evidence"], "model_usage")
        self.assertEqual(result["last_response_at"], "2026-10-05T01:09:00Z")

    def test_stale_response_retains_time_and_is_unconfirmed(self):
        result = self.status([{"token_updated_at": "2026-10-05T01:00:00Z"}])
        self.assertEqual(result["state"], "unknown")
        self.assertEqual(result["last_response_at"], "2026-10-05T01:00:00Z")

    def test_connection_failure_and_later_recovery(self):
        error = {"timestamp": "2026-10-05T01:09:10Z", "category": "conversation", "source": "session", "code": "stream_disconnected"}
        self.assertEqual(self.status(errors=[error])["state"], "error")
        self.assertEqual(self.status([{"token_updated_at": "2026-10-05T01:09:20Z"}], [error])["state"], "response")
        self.assertEqual(self.status([{"token_updated_at": error["timestamp"]}], [error])["state"], "error")

    def test_unrelated_failures_are_not_codex_connection_errors(self):
        for error in ({"category": "tool", "source": "session", "code": "stream_interrupted"}, {"category": "mcp", "source": "codex_core", "cause": "connection_refused"}, {"category": "conversation", "source": "session", "code": "error"}):
            self.assertEqual(self.status(errors=[error | {"timestamp": "2026-10-05T01:09:00Z"}])["state"], "unknown")

    def test_invalid_future_and_naive_times_are_ignored(self):
        for value in (None, "invalid", "2026-10-05T01:11:00Z", "2026-10-05T01:09:00"):
            self.assertEqual(self.status([{"token_updated_at": value}])["state"], "unknown")
        self.assertEqual(self.status([{"token_updated_at": "2026-10-05T01:09:00Z", "metadata_only": True}])["state"], "unknown")

    def test_paused_observation_discards_previous_evidence(self):
        result = self.status([{"token_updated_at": "2026-10-05T01:09:00Z"}], enabled=False)
        self.assertEqual(result["state"], "paused")
        self.assertIsNone(result["last_response_at"])
