"""Run checkout source with the selected interpreter, without installing packages."""
from pathlib import Path
from datetime import datetime, timezone
import json
import os
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from local_activity_monitor.process_lifecycle import cli_lifetime
sys.path.insert(0, str(Path(__file__).resolve().parent))
from watch import stop_child


def write_status(root, status):
    """Store only launcher phases and exit metadata, without arguments or log bodies."""
    directory = root / ".local"
    path = directory / "startup-status.json"
    temporary = None
    try:
        if directory.resolve() != directory or path.is_symlink():
            raise OSError("Startup status path is redirected")
        directory.mkdir(exist_ok=True)
        status["updated_utc"] = datetime.now(timezone.utc).isoformat(timespec="seconds")
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=directory, prefix="startup-status-", suffix=".tmp", delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(status, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
        os.replace(temporary, path)
        return True
    except OSError as error:
        print(f"無法寫入啟動狀態檔 ({type(error).__name__}), 請保留目前視窗的訊息", file=sys.stderr)
        return False
    finally:
        if temporary is not None:
            try:
                temporary.unlink(missing_ok=True)
            except OSError:
                pass


def main(args=None):
    args = sys.argv[1:] if args is None else args
    if sys.version_info < (3, 10):
        print("Python 3.10 or newer is required.", file=sys.stderr)
        return 1
    root = Path(__file__).resolve().parents[1]
    status = {"version": 1, "stage": "initializing", "result": "starting", "pid": os.getpid(), "python": sys.version.split()[0], "runtime": sys.executable}
    reporting = write_status(root, status)
    if reporting:
        print(f"啟動狀態檔: {root / '.local/startup-status.json'}", flush=True)
    def record(stage, result, **values):
        nonlocal reporting
        status.update(stage=stage, result=result, **values)
        if reporting:
            reporting = write_status(root, status)
    try:
        with cli_lifetime():
            record("build_frontend", "starting")
            prepared = subprocess.run([sys.executable, "-X", "utf8", "-I", "-B", str(root / "tools/build_frontend.py"), "--latest"], cwd=root)
            if prepared.returncode:
                record("build_frontend", "failed", exit_code=prepared.returncode)
                return prepared.returncode
            record("watcher", "starting")
            child = subprocess.Popen(
                [sys.executable, "-X", "utf8", "-I", "-B", str(root/"tools/watch.py"), "--watch-stdin", "--codex", "--open", *args],
                cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8",
            )
            try:
                record("watcher", "starting", watcher_pid=child.pid)
                for line in child.stdout:
                    print(line, end="", flush=True)
                    if len(line) > 1024:
                        continue
                    try:
                        event = json.loads(line)
                        value = event.get("status") if isinstance(event, dict) else None
                        location = urlsplit(event.get("url", "")) if value in ("listening", "already_running") else None
                        if location and location.scheme == "http" and location.hostname == "127.0.0.1" and location.port and location.path == "/" and not location.query and not location.fragment and not location.username and not location.password:
                            record("watcher", value, service_status=value, url=location.geturl())
                    except (ValueError, TypeError):
                        pass
                code = child.wait()
                record("watcher", "already_running" if code == 0 and status["result"] == "already_running" else "exited" if code == 0 else "failed", exit_code=code)
                return code
            finally:
                try:
                    stop_child(child)
                finally:
                    child.stdout.close()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        record(status["stage"], "failed", exit_code=1, error_type=type(error).__name__)
        print(f"Startup failed: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        record(status["stage"], "interrupted", exit_code=130)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
