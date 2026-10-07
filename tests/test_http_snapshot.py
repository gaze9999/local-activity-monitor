"""Exercise complete HTTP bodies beyond one 64 KiB transfer buffer."""
import gzip
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading
import unittest
from unittest.mock import MagicMock
from urllib.request import Request, urlopen

from local_activity_monitor.server import handler


class ClientDisconnectTests(unittest.TestCase):
    def request(self):
        request = object.__new__(handler(MagicMock(), 8787))
        request.headers = {}
        request.send_response = MagicMock()
        request.send_header = MagicMock()
        request.end_headers = MagicMock()
        request.wfile = MagicMock()
        request.handle_one_request = lambda: request.reply(200, b"{}")
        return request

    def test_disconnect_during_headers_or_body_closes_the_request(self):
        for error in (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            for phase in ("headers", "body"):
                with self.subTest(error=error.__name__, phase=phase):
                    request = self.request()
                    target = request.end_headers if phase == "headers" else request.wfile.write
                    target.side_effect = error("client disconnected")
                    request.handle()
                    self.assertTrue(request.close_connection)
                    if phase == "headers":
                        request.wfile.write.assert_not_called()

    def test_keepalive_read_disconnect_closes_the_request(self):
        request = self.request()
        def read_request():
            if request.handle_one_request.call_count == 1:
                request.reply(200, b"{}")
                request.close_connection = False
            else:
                raise ConnectionResetError("peer closed")
        request.handle_one_request = MagicMock(side_effect=read_request)
        request.handle()
        self.assertTrue(request.close_connection)
        self.assertEqual(request.handle_one_request.call_count, 2)
        request.wfile.write.assert_called_once_with(b"{}")

    def test_other_failures_are_not_hidden(self):
        for error in (PermissionError, ValueError):
            with self.subTest(error=error.__name__):
                request = self.request()
                request.end_headers.side_effect = error("unexpected failure")
                with self.assertRaises(error):
                    request.handle()


class SnapshotTransportTests(unittest.TestCase):
    def test_large_snapshot_is_complete_with_and_without_gzip(self):
        payload = {"records": [{"id": index, "value": hashlib.sha256(str(index).encode()).hexdigest()} for index in range(3000)]}
        raw = json.dumps(payload, ensure_ascii=False, allow_nan=False).encode("utf-8")
        packed = gzip.compress(raw, compresslevel=1)
        self.assertGreater(len(packed), 65536)

        # Distinguish a local transport restriction from an application defect.
        class BaselineHandler(BaseHTTPRequestHandler):
            def do_GET(self):
                compressed = self.headers.get("Accept-Encoding") == "gzip"
                content = packed if compressed else raw
                self.connection.settimeout(3)
                self.send_response(200)
                self.send_header("Content-Type", "application/json; charset=utf-8")
                self.send_header("Content-Length", str(len(content)))
                if compressed:
                    self.send_header("Content-Encoding", "gzip")
                self.end_headers()
                try:
                    self.wfile.write(content)
                except OSError:
                    pass

            def log_message(self, *args):
                pass

        baseline = ThreadingHTTPServer(("127.0.0.1", 0), BaselineHandler)
        baseline_thread = threading.Thread(target=baseline.serve_forever, daemon=True)
        baseline_thread.start()
        try:
            try:
                for encoding, expected in (("identity", raw), ("gzip", packed)):
                    request = Request(f"http://127.0.0.1:{baseline.server_address[1]}/", headers={"Accept-Encoding": encoding})
                    with urlopen(request, timeout=3) as response:
                        baseline_body = response.read()
                    self.assertEqual(baseline_body, expected)
            except OSError as error:
                self.skipTest(f"Standard-library HTTP baseline unavailable: {type(error).__name__}. Large application bodies remain unverified in this environment")
        finally:
            baseline.shutdown()
            baseline.server_close()
            baseline_thread.join(timeout=3)
        dashboard = MagicMock()
        dashboard.snapshot.return_value = payload
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler(dashboard, 0))
        port = server.server_address[1]
        server.RequestHandlerClass = handler(dashboard, port)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            for encoding in ("identity", "gzip"):
                with self.subTest(encoding=encoding):
                    request = Request(f"http://127.0.0.1:{port}/api/snapshot?window=24h", headers={"Accept-Encoding": encoding})
                    with urlopen(request, timeout=3) as response:
                        body = response.read()
                        self.assertEqual(len(body), int(response.headers["Content-Length"]))
                        if encoding == "gzip":
                            self.assertEqual(response.headers["Content-Encoding"], "gzip")
                            body = gzip.decompress(body)
                    self.assertEqual(body, raw)
                    self.assertEqual(json.loads(body), payload)
                    dashboard.monitor.requested.assert_called_with(200, len(raw), len(gzip.compress(raw, compresslevel=1)) if encoding == "gzip" else len(raw))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=3)


if __name__ == "__main__":
    unittest.main()
