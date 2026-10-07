"""Run checkout source with the selected interpreter, without installing packages."""
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from local_activity_monitor.process_lifecycle import cli_lifetime
sys.path.insert(0, str(Path(__file__).resolve().parent))
from watch import stop_child


def main(args=None):
    args = sys.argv[1:] if args is None else args
    if sys.version_info < (3, 10):
        print("Python 3.10 or newer is required.", file=sys.stderr)
        return 1
    root = Path(__file__).resolve().parents[1]
    try:
        with cli_lifetime():
            prepared = subprocess.run([sys.executable, "-I", "-B", str(root / "tools/build_frontend.py"), "--latest"], cwd=root)
            if prepared.returncode:
                return prepared.returncode
            child = subprocess.Popen(
                [sys.executable, "-I", "-B", str(root/"tools/watch.py"), "--watch-stdin", "--codex", "--open", *args],
                cwd=root, stdin=subprocess.PIPE, text=True,
            )
            try:
                return child.wait()
            finally:
                stop_child(child)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Startup failed: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
