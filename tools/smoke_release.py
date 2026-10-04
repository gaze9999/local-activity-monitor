"""Start only a supplied build against a disposable home and check its HTTP UI."""
import json
from pathlib import Path
from queue import Queue, Empty
import subprocess
import sys
import tempfile
import threading
from urllib.error import HTTPError
from urllib.request import build_opener, ProxyHandler


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
        process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
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
            opener = build_opener(ProxyHandler({}))
            with opener.open(instance["url"], timeout=10) as response:
                body = b""
                while len(body) < 2*1024*1024:
                    chunk = response.read1(65536)
                    if not chunk:
                        break
                    body += chunk
                    if b"</html>" in body:
                        break
                assert b"</html>" in body, "Incomplete bundled HTML"
                html = body.decode("utf-8")
                assert "Content-Security-Policy" in response.headers
                assert "view-skills" in html and "function renderAllowance" in html
            for path in ("api/snapshot?window=24h", "locales.json", "api/instance"):
                with opener.open(instance["url"]+path, timeout=10) as response:
                    value = json.load(response)
                if path.startswith("api/snapshot"):
                    assert value["monitor"]["runtime"]["version"] == __version__
                    assert value["codex"]["threads"] == []
                elif path == "locales.json":
                    assert value["en"] and value["ja"]
            try:
                opener.open(instance["url"]+"../server.py", timeout=10)
            except HTTPError as error:
                assert error.code == 404
            else:
                raise AssertionError("Bundle exposed an arbitrary file")
            assert not (home/"monitoring/jev-monitor.json").exists()
            print("Bundled HTTP, assets, version, empty home and file boundary passed")
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


if __name__ == "__main__":
    main()
