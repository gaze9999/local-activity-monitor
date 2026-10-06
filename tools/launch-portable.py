"""Entry point for the bundled monitor; no installation or source watcher."""
import sys

from local_activity_monitor.server import main


if __name__ == "__main__":
    args = sys.argv[1:]
    browser = [] if "--no-browser" in args else ["--open"]
    controlled = "--watch-stdin" in args
    codex = [] if "--no-codex" in args else ["--codex"]
    raise SystemExit(main([*codex, *browser, *(arg for arg in args if arg not in ("--no-browser", "--watch-stdin", "--no-codex"))], watch_stdin=controlled))
