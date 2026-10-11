import io
import json
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from unittest.mock import MagicMock, patch

from local_activity_monitor.public_api import NoRedirect, PublicApiSource, SOURCES
from local_activity_monitor.server import handler


class PublicApiTests(unittest.TestCase):
    def test_disabled_and_unknown_do_not_request(self):
        source = PublicApiSource()
        with patch.object(source, "_read") as read:
            self.assertEqual(source.query("openai")["health"], "disabled")
            for key in ("unknown", "https://example.test", "../github"):
                with self.assertRaises(ValueError):
                    source.query(key, True)
        read.assert_not_called()

    def test_success_and_failure_cache_and_copy(self):
        source = PublicApiSource()
        with patch.object(source, "_read", side_effect=[{"indicator": "none", "updated_at": None}, OSError("private error")]) as read, patch("local_activity_monitor.public_api.time.monotonic", return_value=0) as clock:
            source.query("openai", True)["indicator"] = "changed"
            self.assertEqual(source.query("openai", True)["indicator"], "none")
            clock.return_value = 60
            result = source.query("openai", True)
            self.assertEqual(result["health"], "error")
            self.assertNotIn("private", json.dumps(result))
            clock.return_value = 119
            self.assertEqual(source.query("openai", True), result)
        self.assertEqual(read.call_count, 2)

    def test_concurrent_requests_share_one_query(self):
        source = PublicApiSource()
        with patch.object(source, "_read", return_value={"indicator": "none", "updated_at": None}) as read:
            with ThreadPoolExecutor(max_workers=8) as workers:
                results = list(workers.map(lambda _: source.query("openai", True), range(8)))
        self.assertTrue(all(result["health"] == "ok" for result in results))
        read.assert_called_once()

    def read_response(self, value):
        response = io.BytesIO(value if isinstance(value, bytes) else json.dumps(value).encode())
        opener = MagicMock()
        opener.open.return_value = response
        with patch("local_activity_monitor.public_api.build_opener", return_value=opener):
            result = PublicApiSource().query("openai", True)
        self.assertEqual(opener.open.call_args.kwargs["timeout"], 4)
        self.assertEqual(opener.open.call_args.args[0].full_url, SOURCES["openai"][1])
        return result

    def test_only_status_and_time_are_selected(self):
        result = self.read_response({"status": {"indicator": "none", "description": "ignored"}, "page": {"updated_at": "2026-10-11T12:00:00Z", "secret": "ignored"}})
        self.assertEqual(result["health"], "ok")
        self.assertEqual(result["indicator"], "none")
        self.assertNotIn("ignored", json.dumps(result))

    def test_bad_format_time_and_oversized_payloads(self):
        for value in ([], {}, {"status": {"indicator": "unknown"}}, {"status": {"indicator": "none"}, "page": {"updated_at": "bad"}}, b"x"*65537, b"not json"):
            with self.subTest(value=str(value)[:50]):
                self.assertEqual(self.read_response(value)["health"], "error")

    def test_redirects_are_not_followed(self):
        self.assertIsNone(NoRedirect().redirect_request(None, None, 302, "", {}, "http://127.0.0.1/private"))

    def test_codex_release_projects_official_tag_and_date(self):
        opener = MagicMock()
        opener.open.return_value = io.BytesIO(json.dumps({"draft": False, "prerelease": False, "tag_name": "rust-v0.160.0", "published_at": "2026-10-11T12:00:00Z", "body": "ignored", "assets": ["ignored"]}).encode())
        with patch("local_activity_monitor.public_api.build_opener", return_value=opener):
            result = PublicApiSource().query("codex", True)
        self.assertEqual(result["version"], "0.160.0")
        self.assertNotIn("ignored", json.dumps(result))

    def test_codex_rejects_prerelease_and_invalid_tag(self):
        for patch_value in ({"prerelease": True}, {"draft": True}, {"tag_name": "untrusted"}, {"published_at": "bad"}):
            value = {"draft": False, "prerelease": False, "tag_name": "rust-v0.160.0", "published_at": "2026-10-11T12:00:00Z"} | patch_value
            opener = MagicMock()
            opener.open.return_value = io.BytesIO(json.dumps(value).encode())
            with patch("local_activity_monitor.public_api.build_opener", return_value=opener):
                self.assertEqual(PublicApiSource().query("codex", True)["health"], "error")

    def test_http_query_validation_and_enablement(self):
        dashboard = MagicMock()
        dashboard.lock = threading.Lock()
        dashboard.observations = {"public_openai": False}
        dashboard.public_api = PublicApiSource()
        from local_activity_monitor.api_queries import ApiQueries
        dashboard.api_queries = ApiQueries({"public_openai": lambda cursor: dashboard.public_api.query("openai", True)}, lambda: None)
        request = object.__new__(handler(dashboard, 8787))
        request.reply = MagicMock()
        with patch.object(dashboard.public_api, "_read", return_value={"indicator": "none", "updated_at": None}) as read:
            from urllib.parse import urlsplit
            for query in ("source=unknown", "source=openai&url=https://example.test", "source=openai&source=github", "source=", ""):
                request.get_local(urlsplit("/api/public-query?"+query))
                self.assertEqual(request.reply.call_args.args[0], 400)
            request.get_local(urlsplit("/api/public-query?source=openai"))
            self.assertEqual(json.loads(request.reply.call_args.args[1])["health"], "ok")
            read.assert_called_once()
            dashboard.observations["public_openai"] = True
            request.get_local(urlsplit("/api/public-query?source=openai"))
            self.assertEqual(json.loads(request.reply.call_args.args[1])["health"], "ok")
            read.assert_called_once()
