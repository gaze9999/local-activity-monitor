"""Check real descendant ownership independently of user services or logs."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
from queue import Queue, Empty
import threading
import time
import unittest

from local_activity_monitor.process_lifecycle import cli_lifetime


ROOT = Path(__file__).resolve().parents[1]


class ProcessLifetimeTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "Windows watcher job ownership")
    def test_watcher_owner_exit_closes_http_descendant_and_preserves_data(self):
        import json
        import socket
        from urllib.parse import urlsplit
        for forced in (False, True):
            with self.subTest(forced=forced), tempfile.TemporaryDirectory(prefix="LAM watcher shutdown ") as folder:
                home = Path(folder)
                retained = home / "retained-metadata.json"
                retained.write_text('{"keep":true}')
                owner = subprocess.Popen([sys.executable, "-X", "utf8", "-I", "-B", str(ROOT / "tools/watch.py"), "--watch-stdin", "--codex-home", str(home), "--port", "0"],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
                lines = Queue()
                def read_lines():
                    for line in owner.stdout:
                        lines.put(line)
                    lines.put(None)
                reader = threading.Thread(target=read_lines, daemon=True)
                reader.start()
                try:
                    deadline = time.monotonic() + 25
                    while True:
                        try:
                            line = lines.get(timeout=max(.01, deadline-time.monotonic()))
                        except Empty:
                            self.fail("Watcher did not start within 25 seconds")
                        if line is None:
                            self.fail("Watcher exited before listening: " + owner.stderr.read())
                        if line.startswith('{"status": "listening"'):
                            address = urlsplit(json.loads(line)["url"])
                            break
                    with socket.create_connection((address.hostname, address.port), timeout=1):
                        pass
                    if forced:
                        owner.terminate()
                    else:
                        owner.stdin.close()
                    owner.wait(timeout=20)
                    if not forced:
                        self.assertEqual(owner.returncode, 0, owner.stderr.read())
                    stopped = False
                    for _ in range(30):
                        try:
                            with socket.create_connection((address.hostname, address.port), timeout=.2):
                                pass
                        except OSError:
                            stopped = True
                            break
                        time.sleep(.1)
                    self.assertTrue(stopped, "Owned HTTP descendant still listens after watcher exit")
                    self.assertEqual(retained.read_text(), '{"keep":true}')
                finally:
                    if owner.poll() is None:
                        owner.kill()
                        owner.wait(timeout=5)
                    reader.join(timeout=5)
                    for stream in (owner.stdin, owner.stdout, owner.stderr):
                        stream.close()

    def test_termination_unwinds_and_restores_signal_handler(self):
        previous = signal.getsignal(signal.SIGTERM)
        with self.assertRaises(KeyboardInterrupt):
            with cli_lifetime():
                signal.getsignal(signal.SIGTERM)(signal.SIGTERM, None)
        self.assertIs(signal.getsignal(signal.SIGTERM), previous)

    @unittest.skipUnless(os.name == "nt", "Windows job ownership")
    def test_normal_and_forced_owner_exit_end_hidden_descendant(self):
        import ctypes
        from ctypes import wintypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
        kernel.WaitForSingleObject.restype = wintypes.DWORD
        kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
        kernel.CloseHandle.restype = wintypes.BOOL
        code = """
import subprocess, sys
sys.path.insert(0, sys.argv[1])
from local_activity_monitor.process_lifecycle import cli_lifetime
with cli_lifetime():
    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],
        creationflags=subprocess.CREATE_NO_WINDOW, stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print(child.pid, flush=True)
    sys.stdin.readline()
"""
        for forced in (False, True):
            with self.subTest(forced=forced):
                owner = subprocess.Popen([sys.executable, "-I", "-B", "-c", code, str(ROOT / "src")],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                handle = None
                try:
                    pid = int(owner.stdout.readline())
                    handle = kernel.OpenProcess(0x100000, False, pid)  # SYNCHRONIZE
                    self.assertTrue(handle)
                    if forced:
                        owner.terminate()
                    else:
                        owner.stdin.write("stop\n")
                        owner.stdin.flush()
                    owner.wait(timeout=15)
                    if not forced:
                        self.assertEqual(owner.returncode, 0, owner.stderr.read())
                    self.assertEqual(kernel.WaitForSingleObject(handle, 10000), 0)
                finally:
                    if owner.poll() is None:
                        owner.kill()
                        owner.wait(timeout=5)
                    for stream in (owner.stdin, owner.stdout, owner.stderr):
                        stream.close()
                    if handle:
                        kernel.CloseHandle(handle)

    def test_parent_control_eof_stops_server_and_preserves_checkpoint_files(self):
        # The synthetic home also prevents collecting real account/session data.
        with tempfile.TemporaryDirectory(prefix="LAM shutdown ") as folder:
            home = Path(folder)
            checkpoint = home / "monitoring/thread-state.json"
            checkpoint.parent.mkdir()
            checkpoint.write_text('{"version":1,"entries":{},"skills":[]}')
            expected = checkpoint.read_bytes()
            code = """
import sys
sys.path.insert(0, sys.argv[1])
from local_activity_monitor.server import main
raise SystemExit(main(['--codex-home', sys.argv[2], '--port', '0'], watch_stdin=True))
"""
            child = subprocess.Popen([sys.executable, "-I", "-B", "-c", code, str(ROOT / "src"), str(home)],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                import json
                import socket
                from urllib.parse import urlsplit
                status = json.loads(child.stdout.readline())
                self.assertEqual(status["status"], "listening")
                child.stdin.close()
                child.wait(timeout=20)
                self.assertEqual(child.returncode, 0, child.stderr.read())
                self.assertEqual(checkpoint.read_bytes(), expected)
                address = urlsplit(status["url"])
                with self.assertRaises(OSError):
                    socket.create_connection((address.hostname, address.port), timeout=1)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=5)
                for stream in (child.stdout, child.stderr):
                    stream.close()


if __name__ == "__main__":
    unittest.main()
