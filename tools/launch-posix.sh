#!/bin/sh
set -eu
ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
MONITOR_INSTALL=no
if [ "${1-}" = --console ]; then shift; fi
if [ "${1-}" = --install-python ]; then MONITOR_INSTALL=yes; shift; fi
if { [ -e "$ROOT_DIR/.venv" ] || [ -L "$ROOT_DIR/.venv" ]; } && [ ! -x "$ROOT_DIR/.venv/bin/python" ]; then
  printf '%s\n' 'Existing .venv is incomplete. Inspect it before retrying; nothing was overwritten.' >&2
  exit 1
fi
MONITOR_SYSTEM=$(uname -s)

find_python() {
  if [ -x "$ROOT_DIR/.venv/bin/python" ]; then
    MONITOR_PYTHON="$ROOT_DIR/.venv/bin/python"
    return 0
  fi
  for MONITOR_PYTHON in python3 python; do
    MONITOR_PATH=$(command -v "$MONITOR_PYTHON") || continue
    # macOS's system shim may request Command Line Tools during a probe.
    if [ "$MONITOR_SYSTEM" = Darwin ] && [ "$MONITOR_PATH" = /usr/bin/python3 ] && ! xcode-select -p >/dev/null 2>&1; then
      continue
    fi
    if "$MONITOR_PYTHON" -I -B -c 'import sys; assert sys.version_info >= (3, 10); import ssl, sqlite3' >/dev/null 2>&1; then
      return 0
    fi
  done
  return 1
}

if ! find_python; then
  printf '%s\n' 'Python 3.10+ with SSL and SQLite support is required.'
  if [ "$MONITOR_INSTALL" != yes ]; then
    printf '%s\n' 'No dependencies were installed. Install Python manually or retry with --install-python.' >&2
    exit 1
  fi
  MONITOR_BREW=''
  case "$MONITOR_SYSTEM" in
    Darwin)
      MONITOR_BREW=$(command -v brew || :)
      if [ -z "$MONITOR_BREW" ]; then
        for MONITOR_PATH in /opt/homebrew/bin/brew /usr/local/bin/brew; do
          if [ -x "$MONITOR_PATH" ]; then MONITOR_BREW=$MONITOR_PATH; break; fi
        done
      fi
      if [ -n "$MONITOR_BREW" ]; then
        MONITOR_INSTALLER=brew
        printf '%s\n' 'This will download Python 3.14 and its prerequisites using your existing Homebrew.' 'Command: brew install python@3.14'
      else
        printf '%s\n' 'Install Python from https://www.python.org/downloads/macos/ and retry. Homebrew will not be installed automatically.' >&2
        exit 1
      fi
      ;;
    Linux)
      if ! command -v apt-get >/dev/null 2>&1; then
        printf '%s\n' 'Install Python 3.10+ with venv/pip using your distribution package manager, then retry. See https://www.python.org/downloads/' >&2
        exit 1
      fi
      MONITOR_INSTALLER=apt
      printf '%s\n' 'This will install Python and venv support from your configured apt repositories.'
      if [ "$(id -u)" = 0 ]; then
        printf '%s\n' 'Command: apt-get install python3 python3-venv'
      elif command -v sudo >/dev/null 2>&1; then
        MONITOR_INSTALLER=sudo-apt
        printf '%s\n' 'Administrator authorization may be required.' 'Command: sudo apt-get install python3 python3-venv'
      else
        printf '%s\n' 'Ask an administrator to install python3 and python3-venv, then retry.' >&2
        exit 1
      fi
      ;;
    *)
      printf '%s\n' 'Install Python from https://www.python.org/downloads/ and retry.' >&2
      exit 1
      ;;
  esac
  printf '%s' 'Install Python now? [y/N] '
  read -r MONITOR_REPLY || MONITOR_REPLY=''
  case "$MONITOR_REPLY" in
    y|Y|yes|YES|Yes) ;;
    *) printf '%s\n' 'Setup cancelled. No installation was performed.'; exit 0 ;;
  esac
  case "$MONITOR_INSTALLER" in
    brew)
      if ! HOMEBREW_NO_AUTO_UPDATE=1 "$MONITOR_BREW" install python@3.14; then
        printf '%s\n' 'Python installation failed. Inspect the installer error above, then retry.' >&2
        exit 1
      fi
      MONITOR_PREFIX=$("$MONITOR_BREW" --prefix python@3.14)
      PATH="$MONITOR_PREFIX/bin:$PATH"
      export PATH
      ;;
    apt|sudo-apt)
      if [ "$MONITOR_INSTALLER" = sudo-apt ]; then
        if sudo apt-get install python3 python3-venv; then MONITOR_INSTALLED=yes; else MONITOR_INSTALLED=no; fi
      else
        if apt-get install python3 python3-venv; then MONITOR_INSTALLED=yes; else MONITOR_INSTALLED=no; fi
      fi
      if [ "$MONITOR_INSTALLED" != yes ]; then
        printf '%s\n' 'Python installation failed. Inspect the installer error above, then retry.' >&2
        exit 1
      fi
      ;;
  esac
  if ! find_python; then
    printf '%s\n' 'Python is still unavailable, below 3.10 or missing SSL/SQLite support. Inspect the installation, then retry.' >&2
    exit 1
  fi
fi
exec "$MONITOR_PYTHON" -I -B "$ROOT_DIR/tools/launch-cli.py" "$@"
