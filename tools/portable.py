"""Entry point for the bundled monitor; no installation or source watcher."""
import sys

from local_activity_monitor.server import main


if __name__ == "__main__":
    args = sys.argv[1:]
    browser = [] if "--no-browser" in args else ["--open"]
    raise SystemExit(main(["--codex", *browser, *(arg for arg in args if arg != "--no-browser")]))
