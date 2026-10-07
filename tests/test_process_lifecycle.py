"""Check real descendant ownership independently of user services or logs."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest

from local_activity_monitor.process_lifecycle import cli_lifetime


ROOT = Path(__file__).resolve().parents[1]


class ProcessLifetimeTests(unittest.TestCase):
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
