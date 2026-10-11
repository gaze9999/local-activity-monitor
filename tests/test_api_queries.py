import threading
import unittest
from unittest.mock import MagicMock, patch

from local_activity_monitor.api_queries import ApiQueries
from local_activity_monitor.server import Dashboard


class ApiQueryTests(unittest.TestCase):
    def test_service_schedule_idle_enable_interval_change_manual_and_stop(self):
        clock = [0.0]
        calls, ready = [], threading.Event()
        def read(cursor):
            calls.append(cursor)
            ready.set()
            return {"health": "ok", "items": [{"id": len(calls)}]}
        queries = ApiQueries({"demo": read}, lambda: None)
        with patch("local_activity_monitor.api_queries.time.monotonic", side_effect=lambda: clock[0]):
            queries.thread.start()
            try:
                self.assertFalse(ready.wait(.03))
                queries.configure({"demo": True}, {"demo": 60})
                self.assertTrue(ready.wait(1));ready.clear()
                self.assertEqual(queries.due["demo"], 60)
                value = queries.snapshot();value["demo"]["items"].clear()
                self.assertEqual(len(queries.snapshot()["demo"]["items"]), 1)
                clock[0] = 30
                queries.query("demo")
                self.assertEqual(queries.due["demo"], 90)
                ready.clear();queries.configure({"demo": True}, {"demo": 120})
                self.assertEqual(queries.due["demo"], 150)
                clock[0] = 149
                with queries.condition:queries.condition.notify_all()
                self.assertFalse(ready.wait(.03))
                clock[0] = 150
                with queries.condition:queries.condition.notify_all()
                self.assertTrue(ready.wait(1));ready.clear()
                queries.configure({"demo": False}, {"demo": 120})
                clock[0] = 1000
                with queries.condition:queries.condition.notify_all()
                self.assertFalse(ready.wait(.03))
                self.assertEqual(len(calls), 3)
            finally:queries.close()
        self.assertFalse(queries.thread.is_alive())

    def test_paged_manual_results_do_not_replace_scheduled_first_page(self):
        queries = ApiQueries({"demo": lambda cursor: {"items": [cursor]}}, MagicMock())
        queries.query("demo")
        queries.query("demo", "older")
        self.assertEqual(queries.snapshot()["demo"]["items"], [None])
        queries.notify.assert_called_once()

    def test_source_failure_waits_for_the_next_deadline(self):
        failed = threading.Event()
        def read(cursor):
            failed.set()
            raise OSError("private")
        queries = ApiQueries({"demo": read}, MagicMock())
        queries.configure({"demo": True}, {"demo": 60});queries.thread.start()
        try:
            self.assertTrue(failed.wait(1))
            with queries.condition:queries.condition.wait_for(lambda: bool(queries.results), 1)
            self.assertEqual(queries.snapshot(), {"demo": {"health": "error"}})
        finally:queries.close()

    def test_settings_validation_preserves_intervals_without_partial_write(self):
        # Construct only the existing settings boundary; refresh is isolated.
        from pathlib import Path
        dashboard = Dashboard(Path("tests/__missing_api_home__").resolve())
        with patch.object(dashboard, "refresh"), patch.object(dashboard.monitor, "event"):
            for value in ({"unknown": 60}, {"public_openai": True}, {"public_openai": 59}, {"public_openai": 86401}, {"public_openai": 1.5}):
                with self.assertRaises(ValueError):dashboard.set_settings({"api_intervals": value, "observations": {"public_openai": True}})
                self.assertFalse(dashboard.observations["public_openai"])
            dashboard.set_settings({"api_intervals": {"public_openai": 600}, "observations": {"public_openai": True}})
            self.assertEqual(dashboard.settings()["api_intervals"]["public_openai"], 600)
            self.assertEqual(dashboard.api_queries.intervals["public_openai"], 600)
        dashboard.api_queries.close()
