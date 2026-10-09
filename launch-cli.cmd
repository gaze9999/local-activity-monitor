@echo off
setlocal DisableDelayedExpansion
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\launch-logged.ps1" --console %*
set "MONITOR_EXIT=%ERRORLEVEL%"
rem Keep the result visible for normal early exits as well as failures.
pause
exit /b %MONITOR_EXIT%
