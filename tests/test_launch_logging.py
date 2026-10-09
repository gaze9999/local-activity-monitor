"""Exercise persistent bootstrap output independently of Python and user data."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from queue import Queue
import threading
import unittest


ROOT = Path(__file__).resolve().parents[1]
POWERSHELL = shutil.which("powershell.exe") if os.name == "nt" else None


@unittest.skipUnless(POWERSHELL, "Windows PowerShell logging")
class LaunchLoggingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="LAM log 中文 with spaces ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / "tools").mkdir()
        for file in ("launch-log.ps1", "launch-logged.ps1"):
            shutil.copyfile(ROOT / "tools" / file, self.root / "tools" / file)

    def run_bootstrap(self, source):
        (self.root / "tools/launch-windows.ps1").write_text(source, encoding="utf-8-sig")
        return subprocess.run([POWERSHELL, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(self.root / "tools/launch-logged.ps1")],
                              capture_output=True, text=True, encoding="utf-8", timeout=20)

    def log(self):
        files = list((self.root / ".local/startup-logs").glob("startup-*.log"))
        self.assertEqual(len(files), 1)
        return files[0].read_text(encoding="utf-8")

    def test_stdout_stderr_stack_and_failure_code_are_saved(self):
        result = self.run_bootstrap('''
[Console]::Out.WriteLine('build stage completed')
[Console]::Error.WriteLine('Traceback (most recent call last):')
[Console]::Error.WriteLine('  File "fixture.py", line 3, in main')
[Console]::Error.WriteLine('ValueError: fixture startup failure')
exit 17
''')
        self.assertEqual(result.returncode, 17, result.stderr + result.stdout)
        log = self.log()
        for value in ("wrapper_start", "bootstrap_start", "build stage completed", "[stderr]", "Traceback", 'File "fixture.py"', "ValueError: fixture startup failure", "bootstrap_exit code=17", "wrapper_exit code=17"):
            self.assertIn(value, log)

    def test_bootstrap_parser_error_is_saved_before_python_can_start(self):
        result = self.run_bootstrap("if (\n")
        self.assertNotEqual(result.returncode, 0)
        log = self.log()
        self.assertIn("[stderr]", log)
        self.assertIn("launch-windows.ps1", log)
        self.assertIn("bootstrap_exit code=", log)
        self.assertIn("wrapper_exit code=", log)

    def test_recognizable_credentials_are_masked_in_log_and_console(self):
        result = self.run_bootstrap('''
[Console]::Out.WriteLine('Authorization: Bearer TEST_PRIVATE_BEARER')
[Console]::Out.WriteLine('api_key="TEST_PRIVATE_KEY"')
[Console]::Error.WriteLine('password=TEST_PRIVATE_PASSWORD')
[Console]::Out.WriteLine('https://TEST_PRIVATE_USER:TEST_PRIVATE_URL_PASSWORD@example.invalid/repo')
[Console]::Out.WriteLine('sk-TEST_PRIVATE_OPENAI_KEY_123456789')
exit 0
''')
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        for text in (self.log(), result.stdout, result.stderr):
            self.assertNotIn("TEST_PRIVATE", text)
        self.assertIn("[已隱藏]", self.log())

    def test_size_is_bounded_without_changing_application_exit(self):
        result = self.run_bootstrap("1..100 | ForEach-Object { [Console]::Out.WriteLine(('x' * 16384)) }; exit 7\n")
        self.assertEqual(result.returncode, 7, result.stderr[-1000:])
        log = self.log()
        self.assertIn("wrapper_exit code=7", log)
        files = list((self.root / ".local/startup-logs").glob("startup-*.log*"))
        self.assertGreater(len(files), 1)
        self.assertLessEqual(len(files), 4)
        for file in files:
            self.assertLessEqual(file.stat().st_size, 1024*1024)

    def test_rotation_continues_during_run_and_removes_old_segments(self):
        harness = self.root / "tools/rotation-fixture.ps1"
        harness.write_text('''
. (Join-Path $PSScriptRoot 'launch-log.ps1')
$null = Initialize-MonitorLog (Split-Path $PSScriptRoot -Parent)
$script:MonitorLogLimit = 512
1..100 | ForEach-Object { Write-MonitorLog ('event_' + $_) }
Write-MonitorLog 'last_runtime_event'
Close-MonitorLog
''', encoding="utf-8-sig")
        result = subprocess.run([POWERSHELL, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(harness)], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        files = list((self.root / ".local/startup-logs").glob("startup-*.log*"))
        self.assertEqual(len(files), 4)
        for file in files:
            self.assertLessEqual(file.stat().st_size, 512)
        self.assertIn("last_runtime_event", self.log())
        self.assertNotIn("event_1\n", "".join(file.read_text(encoding="utf-8") for file in files))

    def test_retention_preserves_unrelated_and_locked_files(self):
        directory = self.root / ".local/startup-logs"
        directory.mkdir(parents=True)
        for index in range(12):
            (directory / f"startup-20000101-000000-{index:03d}-1.log").write_text("owned old log")
            (directory / f"startup-20000101-000000-{index:03d}-1.log.1").write_text("owned old segment")
        unrelated = directory / "notes.log"
        unrelated.write_text("user file")
        locked = directory / "startup-20000101-000000-000-1.log"
        with locked.open("rb"):
            result = self.run_bootstrap("exit 0\n")
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertTrue(locked.exists())
            self.assertTrue(locked.with_name(locked.name + '.1').exists())
            self.assertEqual(len(list(directory.glob("startup-*.log"))), 11)
            self.assertFalse((directory / 'startup-20000101-000000-001-1.log.1').exists())
        self.assertEqual(unrelated.read_text(), "user file")

    def test_unavailable_log_directory_does_not_prevent_bootstrap(self):
        (self.root / ".local").write_text("user file")
        result = self.run_bootstrap("[Console]::Out.WriteLine('application ran'); exit 0\n")
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertIn("application ran", result.stdout)
        self.assertEqual((self.root / ".local").read_text(), "user file")

    def test_forced_logger_exit_keeps_already_flushed_output(self):
        harness = self.root / "tools/flush-fixture.ps1"
        harness.write_text('''
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
. (Join-Path $PSScriptRoot 'launch-log.ps1')
$path = Initialize-MonitorLog (Split-Path $PSScriptRoot -Parent)
Write-MonitorLog 'before_forced_exit'
[Console]::Out.WriteLine($path)
Start-Sleep -Seconds 60
''', encoding="utf-8-sig")
        process = subprocess.Popen([POWERSHELL, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(harness)],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        lines = Queue()
        reader = threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True)
        reader.start()
        try:
            path = Path(lines.get(timeout=10).strip())
            self.assertTrue(path.resolve().is_relative_to(self.root))
            process.terminate()
            process.wait(timeout=5)
            self.assertIn("before_forced_exit", path.read_text(encoding="utf-8"))
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            reader.join(timeout=1)
            process.stdout.close()
            process.stderr.close()


if __name__ == "__main__":
    unittest.main()
