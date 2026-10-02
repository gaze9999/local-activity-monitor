@echo off
setlocal
if exist "%~dp0.venv\Scripts\python.exe" (
  "%~dp0.venv\Scripts\python.exe" -I -B "%~dp0launch.py" %*
  goto done
)
py -3 -c "import sys; assert sys.version_info >= (3, 10)" >nul 2>&1
if not errorlevel 1 (
  py -3 -I -B "%~dp0launch.py" %*
  goto done
)
python -c "import sys; assert sys.version_info >= (3, 10)" >nul 2>&1
if not errorlevel 1 (
  python -I -B "%~dp0launch.py" %*
  goto done
)
echo Python 3.10 or newer is required. Install Python, then retry.
pause
exit /b 1
:done
set "MONITOR_EXIT=%ERRORLEVEL%"
if not "%MONITOR_EXIT%"=="0" (
  echo Monitor could not start. If already running, use its existing browser tab.
  echo Otherwise inspect the error above. See README.md.
  pause
)
exit /b %MONITOR_EXIT%
