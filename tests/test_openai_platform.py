import io
import json
import os
import threading
import unittest
from decimal import Decimal
from unittest.mock import MagicMock, patch
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlsplit

from local_activity_monitor.api_queries import ApiQueries
from local_activity_monitor.openai_platform import OpenAIPlatformSource
from local_activity_monitor.server import handler


class PlatformTests(unittest.TestCase):
    def query(self, source, value=None, error=None):
        opener = MagicMock()
        if error:opener.open.side_effect = error
        else:opener.open.return_value = io.BytesIO(value if isinstance(value, bytes) else json.dumps(value).encode())
        with patch.dict(os.environ, {"OPENAI_API_KEY": "synthetic-key", "OPENAI_ADMIN_KEY": "synthetic-admin"}, clear=True), patch("local_activity_monitor.openai_platform.build_opener", return_value=opener):
            result = OpenAIPlatformSource().query(source)
        return result, opener.open.call_args.args[0]

    def test_missing_credentials_never_send_a_request(self):
        with patch.dict(os.environ, {}, clear=True), patch("local_activity_monitor.openai_platform.build_opener") as opener:
            for source in ("models", "usage", "costs"):
                self.assertEqual(OpenAIPlatformSource().query(source)["health"], "missing_credentials")
            opener.assert_not_called()

    def test_models_only_select_metadata_and_fixed_endpoint(self):
        value, request = self.query("models", {"data": [{"id": "demo-model", "created": 0, "owned_by": "openai", "body": "PRIVATE"}]})
        self.assertEqual(value["health"], "ok")
        self.assertEqual(request.full_url, "https://api.openai.com/v1/models")
        self.assertEqual(value["items"][0]["created_at"], "1970-01-01T00:00:00.000Z")
        self.assertNotIn("PRIVATE", json.dumps(value));self.assertNotIn("synthetic-key", json.dumps(value))

    def test_usage_has_a_bounded_utc_range_and_keeps_missing_values(self):
        value, request = self.query("usage", {"has_more": False, "data": [{"start_time": 1, "end_time": 2, "results": [{"input_tokens": 0, "output_tokens": 5, "num_model_requests": 2, "project_id": "PRIVATE"}]}]})
        self.assertEqual(value["health"], "ok");item = value["items"][0]
        self.assertEqual(item["input_tokens"], 0);self.assertIsNone(item["input_cached_tokens"])
        query = parse_qs(urlsplit(request.full_url).query)
        self.assertEqual(query["limit"], ["7"]);self.assertEqual(query["bucket_width"], ["1d"])
        self.assertLessEqual(int(query["end_time"][0])-int(query["start_time"][0]), 7*86400)
        self.assertNotIn("PRIVATE", json.dumps(value))

    def test_costs_decimal_sum_and_currency_are_preserved(self):
        value, _ = self.query("costs", b'{"has_more":false,"data":[{"start_time":1,"end_time":2,"results":[{"amount":{"currency":"usd","value":0.1}},{"amount":{"currency":"usd","value":0.2}}]}]}')
        self.assertEqual(value["items"][0]["amounts"], {"usd": "0.3"})
        self.assertEqual(OpenAIPlatformSource._project("costs", {"has_more": False, "data": [{"start_time": 1, "end_time": 2, "results": [{"amount": {"currency": "usd", "value": Decimal("-0.01")}}]}]})["items"][0]["amounts"], {"usd": "-0.01"})

    def test_http_failures_retain_only_health(self):
        for code, health in ((401, "unauthorized"), (403, "unauthorized"), (429, "rate_limited"), (500, "error"), (302, "error")):
            value, _ = self.query("models", error=HTTPError("https://api.openai.com", code, "PRIVATE", {}, None))
            self.assertEqual(value["health"], health);self.assertNotIn("PRIVATE", json.dumps(value))

    def test_invalid_format_and_limits(self):
        for value in ({}, {"data": [None]}, {"data": [{"id": "<script>"}]}, b"x"*(1048577)):
            self.assertEqual(self.query("models", value)[0]["health"], "error")
        for value in ({"has_more": False, "data": [{}]}, {"has_more": False, "data": [{}]*8}, {"data": []}):
            self.assertEqual(self.query("usage", value)[0]["health"], "error")

    def test_failure_cache_and_copy(self):
        source = OpenAIPlatformSource()
        with patch.dict(os.environ, {}, clear=True):
            first = source.query("models");first["items"].append("changed")
            self.assertEqual(source.query("models")["items"], [])
        self.assertEqual(len(source.cache), 1)

    def test_http_only_accepts_fixed_sources(self):
        dashboard = MagicMock();dashboard.api_queries = ApiQueries({"platform_models": lambda cursor: {"health": "ok", "items": []}}, lambda: None)
        request = object.__new__(handler(dashboard, 8787));request.reply = MagicMock()
        for query in ("", "source=unknown", "source=models&source=costs", "source=models&url=https://example.test"):
            request.get_local(urlsplit("/api/openai-platform?"+query));self.assertEqual(request.reply.call_args.args[0], 400)
        request.get_local(urlsplit("/api/openai-platform?source=models"));self.assertEqual(json.loads(request.reply.call_args.args[1])["health"], "ok")
