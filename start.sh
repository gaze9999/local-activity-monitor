#!/bin/sh
set -eu
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -x "$ROOT_DIR/.venv/bin/python" ]; then
  exec "$ROOT_DIR/.venv/bin/python" -I -B "$ROOT_DIR/launch.py" "$@"
fi
for MONITOR_PYTHON in python3 python; do
  if command -v "$MONITOR_PYTHON" >/dev/null 2>&1 && "$MONITOR_PYTHON" -c 'import sys; assert sys.version_info >= (3, 10)' >/dev/null 2>&1; then
    exec "$MONITOR_PYTHON" -I -B "$ROOT_DIR/launch.py" "$@"
  fi
done
printf '%s\n' 'Python 3.10 or newer is required. Install Python, then retry.' >&2
exit 1
