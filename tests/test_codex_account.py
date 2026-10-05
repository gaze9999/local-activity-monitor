import io
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

from local_activity_monitor.codex_account import CodexAccountSource, _credits, _limits, _usage, _window


class FakeProcess:
    def __init__(self, responses):
        self.stdin = io.BytesIO()
        self.stdout = io.BytesIO(b"".join(json.dumps(row).encode() + b"\n" for row in responses))
        self.terminated = self.killed = False
        self.sent = b""

    def poll(self):
        return None

    def terminate(self):
        self.terminated = True
        self.sent = self.stdin.getvalue()

    def kill(self):
        self.killed = True

    def wait(self, timeout):
        return 0


def responses(usage_error=None):
    values = [{}, {"account": {"email": "private@example.test", "id": "private-id", "planType": "pro"}},
              {"accountId": "private-id", "rateLimits": {"primary": {"usedPercent": 99}},
               "rateLimitsByLimitId": {"codex": {"primary": {"usedPercent": 0, "windowDurationMins": 300, "resetsAt": 0},
                 "credits": {"hasCredits": False, "unlimited": False, "balance": "0"}}}},
              {"summary": {"lifetimeTokens": 0, "peakDailyTokens": None, "email": "private@example.test"},
               "dailyUsageBuckets": [{"startDate": "2026-10-06", "tokens": 0, "accountId": "private-id"}]}]
    rows = [{"id": index, "result": value} for index, value in enumerate(values)]
    if usage_error:
        rows[-1] = {"id": 3, "error": {"code": usage_error, "message": "private@example.test token-secret"}}
    return rows


class CodexAccountTests(unittest.TestCase):
    def test_disabled_never_launches(self):
        with patch("local_activity_monitor.codex_account.subprocess.Popen") as launch:
            value = CodexAccountSource("/fixture").snapshot()
        launch.assert_not_called()
        self.assertEqual(value["health"], "disabled")
        self.assertIsNone(value["account_usage"])

    def test_whitelist_protocol_cleanup_and_zero(self):
        child = FakeProcess(responses())
        with patch("local_activity_monitor.codex_account._executable", return_value="/bin/codex"), patch("local_activity_monitor.codex_account.subprocess.Popen", return_value=child) as launch:
            value = CodexAccountSource("/fixture").snapshot(True)
        self.assertEqual(value["health"], "ok")
        self.assertEqual(value["limits"]["codex"]["primary"]["used_percent"], 0)
        self.assertEqual(value["limits"]["codex"]["primary"]["remaining_percent"], 100)
        self.assertEqual(value["limits"]["codex"]["primary"]["resets_at"], "1970-01-01T00:00:00+00:00")
        self.assertEqual(value["limits"]["codex"]["primary"]["window_minutes"], 300)
        self.assertEqual(value["limit_id"], "codex")
        self.assertNotIn("secondary", value["limits"]["codex"])
        self.assertEqual(value["credits"], {"has_credits": False, "unlimited": False, "balance": "0"})
        self.assertEqual(value["account_usage"]["summary"]["lifetimeTokens"], 0)
        self.assertNotIn("private", json.dumps(value))
        sent = [json.loads(row) for row in child.sent.splitlines()]
        self.assertEqual([row["method"] for row in sent], ["initialize", "initialized", "account/read", "account/rateLimits/read", "account/usage/read"])
        self.assertFalse(sent[2]["params"]["refreshToken"])
        self.assertFalse(launch.call_args.kwargs["shell"])
        self.assertEqual(launch.call_args.kwargs["env"]["CODEX_HOME"], str(Path("/fixture")))
        self.assertTrue(child.terminated and child.stdin.closed and child.stdout.closed)

    def test_cache_sixty_seconds_and_defensive_copy(self):
        source = CodexAccountSource("/fixture")
        with patch.object(source, "_read", return_value={"health": "ok", "limits": {}}) as read, patch("local_activity_monitor.codex_account.time.monotonic", side_effect=[0, 0, 59, 60, 60]):
            first = source.snapshot(True)
            first["limits"]["modified"] = True
            self.assertEqual(source.snapshot(True)["limits"], {})
            source.snapshot(True)
        self.assertEqual(read.call_count, 2)

    def test_unsupported_method_is_isolated_and_error_text_removed(self):
        child = FakeProcess(responses(-32601))
        with patch("local_activity_monitor.codex_account._executable", return_value="/bin/codex"), patch("local_activity_monitor.codex_account.subprocess.Popen", return_value=child):
            value = CodexAccountSource("/fixture").snapshot(True)
        self.assertEqual(value["health"], "partial")
        self.assertEqual(value["methods"]["account/usage/read"]["health"], "unavailable")
        self.assertIsNone(value["account_usage"])
        self.assertNotIn("secret", json.dumps(value))
        self.assertTrue(child.stdout.closed)

    def test_authentication_failure_is_generic(self):
        child = FakeProcess(responses(-32000))
        with patch("local_activity_monitor.codex_account._executable", return_value="/bin/codex"), patch("local_activity_monitor.codex_account.subprocess.Popen", return_value=child):
            value = CodexAccountSource("/fixture").snapshot(True)
        self.assertEqual(value["methods"]["account/usage/read"]["health"], "error")
        self.assertNotIn("private", json.dumps(value))

    def test_missing_cli_and_launch_failure(self):
        with patch("local_activity_monitor.codex_account._executable", return_value=None):
            self.assertEqual(CodexAccountSource("/fixture").snapshot(True)["health"], "unavailable")
        with patch("local_activity_monitor.codex_account._executable", return_value="/bin/codex"), patch("local_activity_monitor.codex_account.subprocess.Popen", side_effect=OSError("secret")):
            value = CodexAccountSource("/fixture").snapshot(True)
        self.assertEqual(value["health"], "error")
        self.assertNotIn("secret", json.dumps(value))

    def test_output_limit_and_invalid_json_close_child(self):
        for raw in (b"x" * 100, b"x" * (1024 * 1024 + 1)):
            child = FakeProcess([])
            child.stdout = io.BytesIO(raw)
            with patch("local_activity_monitor.codex_account._executable", return_value="/bin/codex"), patch("local_activity_monitor.codex_account.subprocess.Popen", return_value=child):
                value = CodexAccountSource("/fixture").snapshot(True)
            self.assertEqual(value["health"], "error")
            self.assertTrue(child.terminated and child.stdout.closed)

    def test_timeout_and_force_kill_cleanup(self):
        child = FakeProcess(responses())
        child.wait = lambda timeout: (_ for _ in ()).throw(subprocess.TimeoutExpired("codex", timeout))
        with patch("local_activity_monitor.codex_account._executable", return_value="/bin/codex"), patch("local_activity_monitor.codex_account.subprocess.Popen", return_value=child), patch("local_activity_monitor.codex_account.time.monotonic", side_effect=[0, 0, 11, 12]):
            value = CodexAccountSource("/fixture").snapshot(True)
        self.assertEqual(value["health"], "error")
        self.assertTrue(child.killed and child.stdout.closed)

    def test_unknown_null_and_invalid_bounds(self):
        self.assertEqual(_limits({"rateLimitsByLimitId": {}, "rateLimits": {"primary": {"usedPercent": 77}}}), {})
        self.assertIsNone(_window({"usedPercent": True})["used_percent"])
        self.assertIsNone(_window({"usedPercent": -1})["remaining_percent"])
        self.assertEqual(_window({"usedPercent": 110})["remaining_percent"], 0)
        self.assertIsNone(_window({"usedPercent": 10**400})["used_percent"])
        self.assertIsNone(_window({"usedPercent": 0, "resetsAt": 2**63-1})["resets_at"])
        self.assertIsNone(_window(None))
        self.assertEqual(_credits({"hasCredits": False, "unlimited": False, "balance": "secret"}), {"has_credits": False, "unlimited": False})
        self.assertEqual(_usage({"summary": None, "dailyUsageBuckets": None}), {"summary": None, "daily_usage_buckets": None})
        value = _usage({"summary": {"lifetimeTokens": -1}, "dailyUsageBuckets": [{"startDate": "invalid", "tokens": 0}, {"startDate": "2026-10-06", "tokens": True}]})
        self.assertIsNone(value["summary"]["lifetimeTokens"])
        self.assertEqual(value["daily_usage_buckets"], [])

    def test_failed_results_cached_without_disabled_leak(self):
        source = CodexAccountSource("/fixture")
        with patch.object(source, "_read", return_value={"health": "error"}) as read:
            source.snapshot(True)
            self.assertEqual(source.snapshot()["health"], "disabled")
            source.snapshot(True)
        self.assertEqual(read.call_count, 1)


if __name__ == "__main__":
    unittest.main()
