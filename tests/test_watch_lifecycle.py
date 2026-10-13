"""Exercise the source watcher with disposable assets and an empty home."""
import json
import os
from pathlib import Path
from queue import Empty, Queue
import shutil
import subprocess
import sys
import tempfile
from threading import Thread
import time
import unittest
from urllib.request import urlopen


ROOT = Path(__file__).resolve().parents[1]


class WatchLifecycleTests(unittest.TestCase):
    def test_update_failure_duplicate_launch_and_cancelled_build_release_resources(self):
        if not (ROOT / "src/local_activity_monitor/_workbench/manifest.json").is_file():
            self.skipTest("Verified offline assets are required")
        with tempfile.TemporaryDirectory(prefix="lam watcher ") as folder:
            root = Path(folder)
            shutil.copytree(ROOT / "src", root / "src", ignore=shutil.ignore_patterns("__pycache__"))
            shutil.copytree(ROOT / "frontend", root / "frontend")
            (root / "tools").mkdir()
            for name in ("watch.py", "build_frontend.py", "prepare_ui.py"):
                shutil.copyfile(ROOT / "tools" / name, root / "tools" / name)
            shutil.copyfile(ROOT / "workbench-ui.json", root / "workbench-ui.json")
            home = root / "empty home";home.mkdir()
            command = [sys.executable, "-X", "utf8", "-I", "-B", str(root / "tools/watch.py"), "--watch-stdin", "--codex-home", str(home)]
            process = subprocess.Popen([*command, "--port", "0"], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8")
            lines = Queue()
            def read_lines(child, inbox):
                for line in child.stdout:
                    inbox.put(line)
            reader = Thread(target=read_lines, args=(process, lines), daemon=True);reader.start()
            def event(status, timeout=30):
                observed = []
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline:
                    try:
                        line = lines.get(timeout=max(.01, deadline - time.monotonic()))
                    except Empty:
                        break
                    observed.append(line.rstrip()[:200])
                    try:
                        value = json.loads(line)
                    except ValueError:
                        continue
                    if isinstance(value, dict) and value.get("status") == status:
                        return value
                self.fail("Owned watcher did not report " + status + ": " + " / ".join(observed[-10:]))
            location = None
            try:
                location = event("listening")["url"]
                def snapshot():
                    for attempt in range(5):
                        try:
                            with urlopen(location + "api/snapshot?window=24h", timeout=5) as response:
                                return json.load(response)
                        except (ConnectionResetError, ConnectionRefusedError, OSError):
                            if attempt == 4:
                                remaining=[]
                                while not lines.empty():
                                    remaining.append(lines.get_nowait().rstrip()[:200])
                                self.fail("Owned HTTP failed, watcher exit="+str(process.poll())+": "+" / ".join(remaining[-10:]))
                            time.sleep(.2)
                first = snapshot()["monitor"]["pid"]
                port = location.rstrip("/").rsplit(":", 1)[1]
                duplicate = subprocess.Popen([*command, "--port", port], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8")
                try:
                    duplicate_lines = Queue()
                    duplicate_reader = Thread(target=read_lines, args=(duplicate, duplicate_lines), daemon=True);duplicate_reader.start()
                    found = False;deadline = time.monotonic() + 20
                    while time.monotonic() < deadline:
                        try:
                            line = duplicate_lines.get(timeout=max(.01, deadline - time.monotonic()))
                        except Empty:
                            break
                        if '"already_running"' in line:
                            found = True;break
                    self.assertTrue(found)
                    self.assertEqual(duplicate.wait(timeout=10), 0)
                    self.assertEqual(snapshot()["monitor"]["pid"], first)
                finally:
                    if duplicate.poll() is None:
                        duplicate.kill();duplicate.wait(timeout=5)
                    duplicate.stdin.close();duplicate.stdout.close()
                source = root / "src/local_activity_monitor/__init__.py"
                source.write_text(source.read_text() + "\n# isolated update\n")
                location = event("listening")["url"]
                second = snapshot()["monitor"]["pid"]
                self.assertNotEqual(first, second)
                metadata = source.stat()
                os.utime(source, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 2000000000))
                time.sleep(4)
                self.assertEqual(snapshot()["monitor"]["pid"], second)
                server_source = root / "src/local_activity_monitor/server.py"
                server_saved = server_source.read_text(encoding="utf-8")
                def update_failed():
                    deadline = time.monotonic() + 20
                    while time.monotonic() < deadline:
                        line = lines.get(timeout=max(.01, deadline - time.monotonic()))
                        if "Service update failed" in line:
                            return
                    self.fail("Source update did not retain the running service")
                server_source.write_text(server_saved + '\nraise RuntimeError("isolated import failure")\n', encoding="utf-8")
                update_failed()
                self.assertIsNone(process.poll())
                self.assertEqual(snapshot()["monitor"]["pid"], second)
                server_source.write_text(server_saved.replace("    dashboard.preload()", '    raise RuntimeError("isolated startup failure")\n    dashboard.preload()', 1), encoding="utf-8")
                resumed = event("listening")["url"]
                update_failed()
                self.assertEqual(resumed, location)
                self.assertEqual(snapshot()["monitor"]["pid"], second)
                server_source.write_text(server_saved, encoding="utf-8")
                location = event("listening")["url"]
                second = snapshot()["monitor"]["pid"]
                self.assertIsNone(process.poll())
                builder = root / "tools/build_frontend.py"
                saved = builder.read_bytes()
                builder.write_text("raise SystemExit(7)\n")
                deadline = time.monotonic() + 15;failed = False
                while time.monotonic() < deadline:
                    line = lines.get(timeout=max(.01, deadline - time.monotonic()))
                    if "Frontend build failed" in line:
                        failed = True;break
                self.assertTrue(failed)
                self.assertEqual(snapshot()["monitor"]["pid"], second)
                builder.write_bytes(saved)
                location = event("listening")["url"]
                builder.write_text('import time\nprint("owned stalled build", flush=True)\ntime.sleep(60)\n')
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    if "owned stalled build" in lines.get(timeout=max(.01, deadline - time.monotonic())):
                        break
                else:
                    self.fail("Stalled fixture did not start")
                process.stdin.close()
                self.assertEqual(process.wait(timeout=10), 0)
                with self.assertRaises(OSError):
                    urlopen(location, timeout=2)
            finally:
                if not process.stdin.closed:
                    process.stdin.close()
                if process.poll() is None:
                    try:
                        process.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        process.kill();process.wait(timeout=5)
                reader.join(timeout=2);process.stdout.close()


if __name__ == "__main__":
    unittest.main()
