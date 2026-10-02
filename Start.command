#!/bin/sh
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1
sh "$ROOT_DIR/start.sh" "$@"
MONITOR_EXIT=$?
if [ "$MONITOR_EXIT" -ne 0 ]; then
  printf '%s\n' 'Monitor could not start. If already running, use its existing browser tab.' 'Otherwise inspect the error above. See README.md.' 'Press Enter to close.' >&2
  read -r MONITOR_REPLY || :
fi
exit "$MONITOR_EXIT"
