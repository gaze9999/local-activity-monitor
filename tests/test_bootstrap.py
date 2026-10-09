"""Exercise launcher consent/failure paths with fake interpreters and installers."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SH = shutil.which("sh")
if not SH and os.name == "nt":
    candidate = Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/sh.exe"
    if candidate.is_file():
        SH = str(candidate)
if SH and os.name == "nt":
    # Git/bin/sh rewrites PATH and would put host tools before our fakes.
    candidate = Path(SH).parent.parent / "usr/bin/sh.exe"
    if candidate.is_file():
        SH = str(candidate)
POWERSHELL = shutil.which("powershell.exe") if os.name == "nt" else None


class BootstrapFixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="monitor bootstrap ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.ready = self.root / "ready"
        self.install_log = self.root / "install.log"
        self.launch_log = self.root / "launch.log"
        self.env = os.environ.copy()
        self.env.update(MOCK_READY=str(self.ready), MOCK_INSTALL_LOG=str(self.install_log),
                        MOCK_LAUNCH_LOG=str(self.launch_log), MOCK_FAIL="0", MOCK_STAY_MISSING="0",
                        MOCK_LAUNCH_EXIT="0")

    def lines(self, path):
        return path.read_text().splitlines() if path.exists() else []


@unittest.skipUnless(SH, "POSIX shell not installed")
class PosixBootstrapTests(BootstrapFixture):
    def setUp(self):
        super().setUp()
        shutil.copyfile(ROOT / "launch-cli.sh", self.root / "launch-cli.sh")
        (self.root / "tools").mkdir()
        shutil.copyfile(ROOT / "tools/launch-posix.sh", self.root / "tools/launch-posix.sh")
        self.env.update(MOCK_SYSTEM="Linux", MOCK_UID="0")
        # Keep host package managers/interpreters out of discovery. Git's usr/bin
        # supplies dirname on Windows, /usr/bin does so on POSIX hosts.
        utilities = str(Path(SH).resolve().parent) if os.name == "nt" else "/usr/bin:/bin"
        self.env["PATH"] = str(self.bin) + os.pathsep + utilities
        python = '''#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = -c ]; then [ -f "$MOCK_READY" ]; exit $?; fi
done
printf '%s\\n' "$@" > "$MOCK_LAUNCH_LOG"
exit "$MOCK_LAUNCH_EXIT"
'''
        self.script("python3", python)
        self.script("python", python)
        self.script("uname", '#!/bin/sh\nprintf "%s\\n" "$MOCK_SYSTEM"\n')
        self.script("id", '#!/bin/sh\nprintf "%s\\n" "$MOCK_UID"\n')
        self.script("apt-get", '''#!/bin/sh
printf '%s\\n' "$@" > "$MOCK_INSTALL_LOG"
[ "$MOCK_FAIL" = 0 ] || exit 23
if [ "$MOCK_STAY_MISSING" = 0 ]; then : > "$MOCK_READY"; fi
''')
        self.script("sudo", '''#!/bin/sh
printf '%s\\n' "$@" > "$MOCK_INSTALL_LOG.sudo"
exec "$@"
''')
        self.script("brew", '''#!/bin/sh
if [ "$1" = --prefix ]; then printf '%s\\n' "$MOCK_PREFIX"; exit 0; fi
printf '%s\\n' "$@" "$HOMEBREW_NO_AUTO_UPDATE" > "$MOCK_INSTALL_LOG"
[ "$MOCK_FAIL" = 0 ] || exit 23
if [ "$MOCK_STAY_MISSING" = 0 ]; then : > "$MOCK_READY"; fi
''')
        prefix = self.root / "brew prefix"
        (prefix / "bin").mkdir(parents=True)
        (prefix / "bin/python3").write_text(python, newline="\n")
        (prefix / "bin/python3").chmod(0o755)
        self.env["MOCK_PREFIX"] = ("/" + prefix.drive[0].lower() + prefix.as_posix()[2:]) if os.name == "nt" else str(prefix)

    def script(self, name, content):
        target = self.bin / name
        target.write_text(content, newline="\n")
        target.chmod(0o755)

    def run_launcher(self, reply="", args=(), install=True):
        # Send literal LF input. Windows text-mode pipes would convert it to CRLF.
        options = ["--install-python"] if install else []
        result = subprocess.run([SH, str(self.root / "launch-cli.sh"), *options, *args], input=reply.encode(),
                                capture_output=True, env=self.env, timeout=15)
        return subprocess.CompletedProcess(result.args, result.returncode, result.stdout.decode(), result.stderr.decode())

    def test_ready_environment_skips_install_and_preserves_arguments(self):
        self.ready.touch()
        result = self.run_launcher("y\n", ["--codex-home", "folder with spaces"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.install_log.exists())
        self.assertEqual(self.lines(self.launch_log)[-2:], ["--codex-home", "folder with spaces"])
        self.assertNotIn("Install Python", result.stdout)

    def test_default_missing_runtime_never_prompts_or_installs(self):
        result = self.run_launcher("y\n", install=False)
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("Install Python now?", result.stdout)
        self.assertFalse(self.install_log.exists())
        self.assertFalse(self.launch_log.exists())

    def test_decline_empty_or_eof_never_installs_or_launches(self):
        for reply in ("n\n", "\n", ""):
            with self.subTest(reply=reply):
                result = self.run_launcher(reply)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("cancelled", result.stdout)
                self.assertFalse(self.install_log.exists())
                self.assertFalse(self.launch_log.exists())

    def test_apt_consent_installs_then_launches(self):
        result = self.run_launcher("yes\n", ["--port", "8790"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.lines(self.install_log), ["install", "python3", "python3-venv"])
        self.assertEqual(self.lines(self.launch_log)[-2:], ["--port", "8790"])

    def test_next_start_reuses_installed_runtime(self):
        self.assertEqual(self.run_launcher("y\n").returncode, 0)
        self.install_log.unlink()
        result = self.run_launcher("y\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.install_log.exists())
        self.assertNotIn("Install Python", result.stdout)

    def test_non_root_apt_uses_sudo_after_consent(self):
        self.env["MOCK_UID"] = "1000"
        result = self.run_launcher("y\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.lines(Path(str(self.install_log) + ".sudo")), ["apt-get", "install", "python3", "python3-venv"])
        self.assertIn("Administrator", result.stdout)

    def test_brew_install_and_prefix_preserve_arguments(self):
        self.env["MOCK_SYSTEM"] = "Darwin"
        result = self.run_launcher("Y\n", ["--codex-home", "folder with spaces"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.lines(self.install_log), ["install", "python@3.14", "1"])
        self.assertEqual(self.lines(self.launch_log)[-2:], ["--codex-home", "folder with spaces"])

    def test_failed_install_stops_before_launch(self):
        for system in ("Linux", "Darwin"):
            with self.subTest(system=system):
                self.env.update(MOCK_SYSTEM=system, MOCK_FAIL="1")
                result = self.run_launcher("y\n")
                self.assertEqual(result.returncode, 1)
                self.assertIn("installation failed", result.stderr)
                self.assertFalse(self.launch_log.exists())

    def test_install_success_without_usable_python_stops(self):
        self.env["MOCK_STAY_MISSING"] = "1"
        result = self.run_launcher("y\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("still unavailable", result.stderr)
        self.assertFalse(self.launch_log.exists())

    def test_unsupported_platform_prints_manual_source(self):
        self.env["MOCK_SYSTEM"] = "Other"
        result = self.run_launcher("y\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("https://www.python.org/downloads/", result.stderr)
        self.assertFalse(self.install_log.exists())

    def test_application_failure_exit_code_is_preserved(self):
        self.ready.touch()
        self.env["MOCK_LAUNCH_EXIT"] = "17"
        self.assertEqual(self.run_launcher().returncode, 17)

    def test_incomplete_environment_stops_before_install(self):
        (self.root / ".venv").mkdir()
        result = self.run_launcher("y\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("incomplete", result.stderr)
        self.assertTrue((self.root / ".venv").is_dir())
        self.assertFalse(self.install_log.exists())
        self.assertFalse(self.launch_log.exists())


@unittest.skipUnless(POWERSHELL, "Windows PowerShell not installed")
class WindowsBootstrapTests(BootstrapFixture):
    def setUp(self):
        super().setUp()
        self.env.update(MOCK_MANAGER="winget", MOCK_REPLY="y", MOCK_INSTALL="1", MOCK_PROBE_LOG=str(self.root / "probe.log"))
        python = self.bin / "python.cmd"
        python.write_text(r'''@echo off
if "%~1"=="-3" shift /1
if "%~3"=="-c" goto probe
:args
if "%~1"=="" exit /b %MOCK_LAUNCH_EXIT%
>>"%MOCK_LAUNCH_LOG%" echo(%~1
shift
goto args
:probe
>>"%MOCK_PROBE_LOG%" echo(%PYTHON_MANAGER_AUTOMATIC_INSTALL%
if not exist "%MOCK_READY%" exit /b 1
set "MOCK_PYTHON=%~dp0python.cmd"
set "MOCK_PYTHON=%MOCK_PYTHON:\=/%"
echo("%MOCK_PYTHON%"
exit /b 0
''', newline="\r\n")
        installer = self.bin / "installer.cmd"
        installer.write_text('''@echo off
:args
if "%~1"=="" goto installed
>>"%MOCK_INSTALL_LOG%" echo(%~1
shift
goto args
:installed
if not "%MOCK_FAIL%"=="0" exit /b 23
if "%MOCK_STAY_MISSING%"=="0" type nul > "%MOCK_READY%"
exit /b 0
''', newline="\r\n")
        # Restrict discovery to fixture applications even after PATH refresh.
        def quote(value):
            return "'" + str(value).replace("'", "''") + "'"
        harness = self.root / "bootstrap.ps1"
        harness.write_text(f'''. {quote(ROOT / 'tools/launch-windows.ps1')}
function Get-Command {{
    param($Name, $CommandType, $ErrorAction)
    if ($Name -in @('py', 'python', 'python3')) {{ return [pscustomobject]@{{ Source = {quote(python)} }} }}
    if ($Name -eq $env:MOCK_MANAGER) {{ return [pscustomobject]@{{ Source = {quote(installer)} }} }}
    return $null
}}
function Read-Host {{ param($Prompt) return $env:MOCK_REPLY }}
exit (Start-Monitor {quote(self.root)} @('--codex-home', 'folder with spaces') ($env:MOCK_INSTALL -eq '1'))
''')
        self.harness = harness

    def run_launcher(self):
        return subprocess.run([POWERSHELL, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(self.harness)],
                              capture_output=True, text=True, env=self.env, timeout=15)

    def test_ready_environment_skips_install_and_prevents_probe_downloads(self):
        self.ready.touch()
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.install_log.exists())
        self.assertEqual(self.lines(self.launch_log)[-2:], ["--codex-home", "folder with spaces"])
        self.assertEqual(self.lines(self.root / "probe.log"), ["false"])

    def test_default_missing_runtime_never_prompts_or_installs(self):
        self.env["MOCK_INSTALL"] = "0"
        result = self.run_launcher()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn("Install Python now?", result.stdout)
        self.assertFalse(self.install_log.exists())
        self.assertFalse(self.launch_log.exists())

    def test_multiple_discovery_paths_try_each_without_installing(self):
        self.ready.touch()
        unavailable = self.bin / "unavailable.cmd"
        unavailable.write_text("@exit /b 1\n", newline="\r\n")
        harness = self.harness.read_text()
        command = "[pscustomobject]@{ Source = '" + str(unavailable).replace("'", "''") + "' }"
        harness = harness.replace("return [pscustomobject]@{ Source = ", "return " + command + ", [pscustomobject]@{ Source = ", 1)
        self.harness.write_text(harness)
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.install_log.exists())
        self.assertEqual(self.lines(self.launch_log)[-2:], ["--codex-home", "folder with spaces"])
        self.assertEqual(self.lines(self.root / "probe.log"), ["false"])

    def test_decline_or_empty_never_installs_or_launches(self):
        for reply in ("n", ""):
            with self.subTest(reply=reply):
                self.env["MOCK_REPLY"] = reply
                result = self.run_launcher()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("cancelled", result.stdout)
                self.assertFalse(self.install_log.exists())
                self.assertFalse(self.launch_log.exists())

    def test_winget_consent_uses_exact_user_install_and_launches(self):
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.lines(self.install_log), ["install", "--id", "Python.Python.3.14", "--exact", "--source", "winget", "--scope", "user", "--accept-source-agreements", "--accept-package-agreements"])
        self.assertEqual(self.lines(self.launch_log)[-2:], ["--codex-home", "folder with spaces"])
        self.assertTrue(all(line == "false" for line in self.lines(self.root / "probe.log")))

    def test_existing_python_manager_is_preferred(self):
        self.env["MOCK_MANAGER"] = "pymanager"
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.lines(self.install_log), ["install", "3.14"])

    def test_failed_install_stops_before_launch(self):
        self.env["MOCK_FAIL"] = "1"
        result = self.run_launcher()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("installation failed", result.stdout)
        self.assertFalse(self.launch_log.exists())

    def test_install_success_without_usable_python_stops(self):
        self.env["MOCK_STAY_MISSING"] = "1"
        result = self.run_launcher()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("still unavailable", result.stdout)
        self.assertFalse(self.launch_log.exists())

    def test_missing_installer_prints_manual_source(self):
        self.env["MOCK_MANAGER"] = "none"
        result = self.run_launcher()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("https://www.python.org/downloads/windows/", result.stdout)
        self.assertFalse(self.install_log.exists())

    def test_application_failure_exit_code_is_preserved(self):
        self.ready.touch()
        self.env["MOCK_LAUNCH_EXIT"] = "17"
        self.assertEqual(self.run_launcher().returncode, 17)

    def test_incomplete_environment_stops_before_install(self):
        (self.root / ".venv").mkdir()
        result = self.run_launcher()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("incomplete", result.stdout)
        self.assertTrue((self.root / ".venv").is_dir())
        self.assertFalse(self.install_log.exists())
        self.assertFalse(self.launch_log.exists())

    def test_cmd_entry_point_preserves_arguments_from_space_path(self):
        # Real interpreter, isolated inert entry point. No LAM service or install.
        checkout = self.root / "來源 🐍 with spaces"
        checkout.mkdir()
        shutil.copyfile(ROOT / "launch-cli.cmd", checkout / "launch-cli.cmd")
        (checkout / "tools").mkdir()
        shutil.copyfile(ROOT / "tools/launch-windows.ps1", checkout / "tools/launch-windows.ps1")
        for file in ("launch-logged.ps1", "launch-log.ps1"):
            shutil.copyfile(ROOT / "tools" / file, checkout / "tools" / file)
        arguments = ["--codex-home", "資料夾 🐍 with spaces"]
        command = subprocess.list2cmdline([str(checkout / "launch-cli.cmd"), *arguments])
        for code in (0, 17):
            with self.subTest(exit_code=code):
                (checkout / "tools/launch-cli.py").write_text(f"import json, sys\nprint(json.dumps(sys.argv[1:]))\nsys.exit({code})\n")
                result = subprocess.run('"' + os.environ["COMSPEC"] + '" /d /s /c "' + command + '"', input="", capture_output=True,
                                        text=True, encoding='utf-8', env=self.env, timeout=15)
                self.assertEqual(result.returncode, code, result.stderr + result.stdout)
                output = next(line for line in result.stdout.splitlines() if line.startswith("["))
                self.assertEqual(json.loads(output), arguments)
                self.assertIn(f"退出碼 {code}", result.stdout)

    def test_powershell_entry_points_preserve_mode_and_unicode_arguments(self):
        for entry, mode in (("launch-cli.ps1", "--console"),):
            with self.subTest(entry=entry):
                checkout = self.root / entry.replace(".ps1", " 資料夾 🐍 with spaces")
                (checkout / "tools").mkdir(parents=True)
                shutil.copyfile(ROOT / entry, checkout / entry)
                for file in ("launch-logged.ps1", "launch-log.ps1"):
                    shutil.copyfile(ROOT / "tools" / file, checkout / "tools" / file)
                (checkout / "tools/launch-windows.ps1").write_text("[IO.File]::WriteAllText($env:MOCK_LAUNCH_LOG, (ConvertTo-Json -InputObject @($args) -Compress), [Text.Encoding]::UTF8)\nexit 0\n")
                arguments = ["--fixture", "資料夾 🐍 with spaces"]
                result = subprocess.run([POWERSHELL, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(checkout / entry), *arguments], capture_output=True, text=True, env=self.env, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(self.launch_log.read_text(encoding="utf-8-sig")), [mode, *arguments])


if __name__ == "__main__":
    unittest.main()
