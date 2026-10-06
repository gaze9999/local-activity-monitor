#!/bin/sh
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1
exec sh "$ROOT_DIR/tools/launch-posix.sh" --console "$@"
