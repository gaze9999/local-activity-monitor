"""Confirm first-time setup, then run the repository-local installation."""
from pathlib import Path
import subprocess
import sys


def main(args=None):
    args = sys.argv[1:] if args is None else args
    if sys.version_info < (3, 10):
        print("Python 3.10 or newer is required.", file=sys.stderr)
        return 1
    root = Path(__file__).resolve().parent
    environment = root / ".venv"
    python = environment / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    if environment.exists() and not python.is_file():
        print("Existing .venv is incomplete. Inspect it before retrying; nothing was overwritten.", file=sys.stderr)
        return 1
    try:
        ready = python.is_file() and subprocess.run(
            [str(python), "-I", "-B", "-c", "import sys; assert sys.version_info >= (3, 10); import local_activity_monitor.server"],
            cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        ).returncode == 0
        if not ready:
            print("First-time setup will create/use this folder's .venv and install requirements.txt.")
            print("Build tools may be downloaded. System Python packages will not be changed.")
            try:
                answer = input("Install and start now? [y/N] ").strip().lower()
            except EOFError:
                answer = ""
            if answer not in ("y", "yes"):
                print("Setup cancelled. No installation was performed.")
                return 0
            if not python.is_file():
                subprocess.run([sys.executable, "-m", "venv", str(environment)], cwd=root, check=True)
            subprocess.run(
                [str(python), "-I", "-m", "pip", "--disable-pip-version-check", "install", "-r", "requirements.txt"],
                cwd=root, check=True,
            )
        return subprocess.run(
            [str(python), "-I", "-B", "-m", "local_activity_monitor", "--codex", "--open", *args],
            cwd=root,
        ).returncode
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"Setup or startup failed: {error}", file=sys.stderr)
        print("Inspect the error and retry. Existing files were kept.", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
