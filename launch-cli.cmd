@echo off
setlocal DisableDelayedExpansion
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\launch-windows.ps1" --console %*
set "MONITOR_EXIT=%ERRORLEVEL%"
if not "%MONITOR_EXIT%"=="0" (
  echo Monitor could not start. If already running, use its existing browser tab.
  echo Otherwise inspect the error above. See README.md.
  pause
)
exit /b %MONITOR_EXIT%
