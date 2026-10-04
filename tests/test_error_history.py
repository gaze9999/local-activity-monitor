from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.error_history import ErrorHistory, error_identity
from local_activity_monitor.server import Dashboard


def event(hours=0, **fields):
    return {"timestamp": (datetime.now(timezone.utc)-timedelta(hours=hours)).isoformat(), "severity": "error", "source": "session", "category": "conversation", "code": "submit_failed", **fields}


class ErrorHistoryTests(unittest.TestCase):
    def test_reload_prune_deduplicate_and_project_private_fields(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/"history.json"
            history = ErrorHistory(path)
            value = event(message="PRIVATE", thread_name="PRIVATE", command="PRIVATE", error_type="TypeError")
            history.update([value, event(25), value])
            self.assertEqual(len(history.snapshot()), 1)
            self.assertNotIn("PRIVATE", path.read_text())
            self.assertEqual(ErrorHistory(path).snapshot(), history.snapshot())
            stamp = path.stat().st_mtime_ns
            history.update([value])
            self.assertEqual(path.stat().st_mtime_ns, stamp)
            self.assertEqual(error_identity(value), error_identity(history.snapshot()[0]))

    def test_caps_and_write_failure_preserve_memory(self):
        with tempfile.TemporaryDirectory() as folder:
            history = ErrorHistory(Path(folder)/"history.json")
            history.LIMIT = 3
            stamp = datetime.now(timezone.utc)
            history.update([event(timestamp=(stamp-timedelta(seconds=8-index)).isoformat(), index=index) for index in range(8)])
            self.assertEqual(len(history.snapshot()), 3)
            self.assertLessEqual(history.path.stat().st_size, history.BYTE_LIMIT)
            with patch("local_activity_monitor.error_history.os.replace", side_effect=PermissionError("PRIVATE")):
                history.update([event(timestamp=stamp.isoformat(), index=99)])
            self.assertEqual(history.health, "unavailable")
            self.assertTrue(any(value["index"] == 99 for value in history.snapshot()))
            self.assertNotIn("PRIVATE", history.error_type)

    def test_dashboard_restart_retains_observed_errors(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            first = Dashboard(home, codex=True)
            now = datetime.now(timezone.utc).isoformat()
            first.diagnostics.desktop_line(f"{now} error [mcp-client] failed method=run")
            first.refresh()
            second = Dashboard(home, codex=True); second.refresh()
            self.assertEqual(second.snapshot("24h")["errors"]["categories"]["mcp"], 1)
            self.assertTrue(any(row["source"] == "codex_desktop" for row in second.logs()["entries"]))
            second.set_settings({"observations": {"errors": False}})
            self.assertEqual(second.snapshot("all")["errors"]["events"], [])


if __name__ == "__main__":
    unittest.main()
