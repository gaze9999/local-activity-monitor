"""Start only a supplied build against a disposable home and check its HTTP UI."""
import json
import gzip
import os
from http.client import HTTPConnection
from pathlib import Path
from queue import Queue, Empty
import subprocess
import sys
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.parse import urlsplit


def read_response(connection, url):
    # Match browser compression and retry only transient loopback transport failures.
    for attempt in range(5):
        try:
            route = urlsplit(url)
            connection.request("GET", route.path + ("?" + route.query if route.query else ""), headers={"Accept-Encoding": "gzip"})
            response = connection.getresponse()
            body = response.read()
            if response.status >= 400:
                raise HTTPError(url, response.status, response.reason, response.headers, None)
            return response.headers, gzip.decompress(body) if response.headers.get("Content-Encoding") == "gzip" else body
        except (ConnectionResetError, TimeoutError):
            connection.close()
            if attempt == 4:
                raise
            time.sleep(.2)


def main():
    root = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(root/"src"))
    from local_activity_monitor import __version__
    executable = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory() as directory:
        home = Path(directory)
        args = [str(executable), "--codex-home", str(home), "--port", "0", "--no-browser"]
        if executable.suffix == ".py":
            args.insert(0, sys.executable)
        env = dict(os.environ, CODEX_HOME=str(home), LOCALAPPDATA=str(home), XDG_DATA_HOME=str(home), HOME=str(home))
        process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        connection = None
        lines = Queue()
        threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True).start()
        try:
            try:
                line = lines.get(timeout=30)
            except Empty:
                raise RuntimeError("Bundled service did not start within 30 seconds") from None
            if not line:
                raise RuntimeError("Bundled startup failed: " + process.stderr.read(4096))
            instance = json.loads(line)
            assert instance["status"] == "listening", instance
            location = urlsplit(instance["url"])
            assert location.hostname == "127.0.0.1", "Release smoke stays on loopback"
            connection = HTTPConnection("127.0.0.1", location.port, timeout=5)
            headers, body = read_response(connection, instance["url"])
            assert b"</html>" in body, "Incomplete bundled HTML"
            html = body.decode("utf-8")
            assert "Content-Security-Policy" in headers
            assert "view-skills" in html and "function renderAllowance" in html
            assert 'href="/favicon.svg"' in html and 'href="/favicon.ico"' in html
            for path, mime in (("favicon.svg", "image/svg+xml"), ("favicon.ico", "image/vnd.microsoft.icon")):
                icon_headers, icon = read_response(connection, instance["url"]+path)
                assert icon_headers["Content-Type"] == mime
                assert (b"<svg" in icon if path.endswith(".svg") else icon[:4] == b"\0\0\x01\0"), "Invalid bundled icon"
            for path in ("api/snapshot?window=24h", "locales.json", "api/instance"):
                _, body = read_response(connection, instance["url"]+path)
                value = json.loads(body)
                if path.startswith("api/snapshot"):
                    assert value["monitor"]["version"] == __version__
                    assert value["codex"]["threads"] == []
                elif path == "locales.json":
                    assert value["en"] and value["ja"]
            try:
                read_response(connection, instance["url"]+"../server.py")
            except HTTPError as error:
                assert error.code == 404
            else:
                raise AssertionError("Bundle exposed an arbitrary file")
            assert not (home/"monitoring/jev-monitor.json").exists()
            print("Bundled HTTP, assets, version, empty home and file boundary passed")
        finally:
            if connection is not None:
                connection.close()
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            for stream in (process.stdout, process.stderr):
                stream.close()


if __name__ == "__main__":
    main()
