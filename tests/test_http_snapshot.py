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


class SnapshotTransportTests(unittest.TestCase):
    def test_large_snapshot_is_complete_with_and_without_gzip(self):
        payload = {"records": [{"id": index, "value": hashlib.sha256(str(index).encode()).hexdigest()} for index in range(3000)]}
        raw = json.dumps(payload, ensure_ascii=False, allow_nan=False).encode("utf-8")
        self.assertGreater(len(gzip.compress(raw, compresslevel=1)), 65536)

        # Distinguish a local transport restriction from an application defect.
        class BaselineHandler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.connection.settimeout(3)
                self.send_response(200)
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                try:
                    self.wfile.write(raw)
                except OSError:
                    pass

            def log_message(self, *args):
                pass

        baseline = ThreadingHTTPServer(("127.0.0.1", 0), BaselineHandler)
        baseline_thread = threading.Thread(target=baseline.serve_forever, daemon=True)
        baseline_thread.start()
        try:
            try:
                with urlopen(f"http://127.0.0.1:{baseline.server_address[1]}/", timeout=3) as response:
                    baseline_body = response.read()
                self.assertEqual(baseline_body, raw)
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
