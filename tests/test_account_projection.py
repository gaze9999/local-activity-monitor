import tempfile
from pathlib import Path
import unittest
from unittest.mock import MagicMock

from local_activity_monitor.server import Dashboard


class AccountProjectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.dashboard = Dashboard(Path(self.temp.name))
        self.source = {"health": "ok", "updated_at": "2026-10-06T01:00:00Z", "limit_id": "codex", "plan_type": "pro", "credits": None,
                       "limits": {"codex": {"limit_id": "codex", "primary": {"window_minutes": 10080, "remaining_percent": 0, "used_percent": 100, "resets_at": None}, "secondary": None}},
                       "account_usage": {"summary": {"lifetimeTokens": 0}, "daily_usage_buckets": None},
                       "methods": {"account/read": {"health": "ok"}, "account/rateLimits/read": {"health": "ok"}, "account/usage/read": {"health": "ok"}}}

    def test_default_disables_official_reader(self):
        self.dashboard.account.snapshot = MagicMock(return_value={"health": "disabled", "limits": {}, "methods": {"account/rateLimits/read": {"health": "disabled"}}})
        self.dashboard.refresh()
        self.dashboard.account.snapshot.assert_called_once_with(False)
        self.assertFalse(self.dashboard.default_settings["observations"]["codex_account"])

    def test_official_bucket_preserves_long_primary_zero_and_null_secondary(self):
        self.dashboard.account.snapshot = MagicMock(return_value=self.source)
        self.dashboard.observations["codex_account"] = True
        self.dashboard.refresh()
        for snapshot in self.dashboard.cache.values():
            usage = snapshot["codex"]["usage"]
            self.assertEqual(usage["source"], "codex_app_server")
            self.assertEqual(usage["limits"], [self.source["limits"]["codex"]["primary"]])
            self.assertEqual(usage["limits"][0]["remaining_percent"], 0)
            self.assertIsNone(usage["credits"])
            self.assertEqual(snapshot["codex"]["account"]["account_usage"]["summary"]["lifetimeTokens"], 0)
        source = self.dashboard.sources(self.dashboard.cache["all"])["account_api"]
        self.assertEqual(source["mode"], "read_only")
        self.assertEqual(source["limits"]["cache_seconds"], 60)

    def test_usage_switch_disables_reader(self):
        self.dashboard.observations.update(codex_account=True, usage=False)
        self.dashboard.account.snapshot = MagicMock(return_value={"health": "error", "limits": {}, "methods": {"account/rateLimits/read": {"health": "error"}}})
        self.dashboard.refresh()
        self.dashboard.account.snapshot.assert_called_once_with(False)
        self.assertNotIn("usage", self.dashboard.cache["all"]["codex"])

    def test_failed_account_read_keeps_local_quota(self):
        from local_activity_monitor.collectors import CodexCollector
        self.dashboard.observations.update(codex=True, codex_account=True)
        self.dashboard.codex = CodexCollector(Path(self.temp.name))
        local = {"source": "codex", "limit_id": "codex", "limits": [{"remaining_percent": 55, "window_minutes": 10080}]}
        self.dashboard.codex.snapshot = MagicMock(return_value={"health": "ok", "threads": [], "usage": local})
        self.dashboard.account.snapshot = MagicMock(return_value={"health": "error", "limits": {}, "methods": {"account/rateLimits/read": {"health": "error"}}})
        self.dashboard.refresh()
        self.dashboard.account.snapshot.assert_called_once_with(True)
        self.assertEqual(self.dashboard.cache["all"]["codex"]["usage"], local)


if __name__ == "__main__":
    unittest.main()
