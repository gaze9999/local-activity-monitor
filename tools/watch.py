"""Build checkout assets and restart only this child when source files change."""
from pathlib import Path
import hashlib
import json
from queue import Empty, Queue
import subprocess
import sys
import time
from threading import Event, Thread

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from local_activity_monitor.process_lifecycle import cli_lifetime
from local_activity_monitor.ui_assets import development_source


def signature(root):
    result = []
    package = root / "src/local_activity_monitor"
    paths = [path for path in package.rglob("*.py") if path.relative_to(package).parts[0] not in {"_workbench", "_web"}]
    paths.extend(path for path in (root/"frontend").glob("*") if path.is_file())
    paths.extend((root / "tools/build_frontend.py", root / "workbench-ui.json"))
    paths.append(root / ".local/workbench-ui-development.json")
    selected = development_source(root)
    if selected:
        paths.extend((selected / "src" / name) for name in ("workbench-ui.js", "workbench-ui.css", "workbench-ui.mjs", "workbench-loader.js"))
    for path in sorted(paths):
        try:
            result.append((str(path), hashlib.sha256(path.read_bytes()).digest()))
        except OSError:
            continue
    return tuple(result)


def stop_child(child):
    try:
        if child.poll() is not None:
            return
        try:
            if child.stdin:
                child.stdin.write("restart\n")
                child.stdin.flush()
            else:
                child.terminate()
        except (OSError, BrokenPipeError):
            if child.poll() is None:
                child.terminate()
        try:
            child.wait(timeout=15)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
    finally:
        if child.stdin:
            try:
                child.stdin.close()
            except OSError:
                # The child may already have closed its control pipe on interrupt.
                pass


def build_frontend(root, *, latest=False, stop=None, timeout=180):
    command = [sys.executable, "-X", "utf8", "-I", "-B", str(root / "tools/build_frontend.py")]
    if latest:
        command.append("--latest")
    # A concurrent control-pipe reader can block Windows child initialization.
    child = subprocess.Popen(command, cwd=root, stdin=subprocess.DEVNULL)
    deadline = time.monotonic() + timeout
    try:
        while True:
            if stop is not None and stop.is_set():
                return 130
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                print("Frontend build timed out.", file=sys.stderr, flush=True)
                return 124
            try:
                return child.wait(timeout=min(.2, remaining))
            except subprocess.TimeoutExpired:
                continue
    finally:
        stop_child(child)


class ServiceProcess:
    """Own one service, its output reader and private handoff notifications."""
    def __init__(self, root, args, prepared=False):
        self.events = Queue()
        mode = "--prepare-child" if prepared else "--serve-child"
        self.process = subprocess.Popen([sys.executable, "-X", "utf8", "-I", "-B", str(root / "tools/watch.py"), mode, *args], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
        self.reader = Thread(target=self.read_output, daemon=True)
        self.reader.start()

    def read_output(self):
        try:
            for line in self.process.stdout:
                try:
                    value = json.loads(line)
                except ValueError:
                    value = None
                status = value.get("status") if isinstance(value, dict) else None
                if status:
                    self.events.put(status)
                if status not in {"source_ready", "paused"}:
                    print(line, end="", flush=True)
        finally:
            self.events.put(None)

    def send(self, command):
        self.process.stdin.write(command + "\n")
        self.process.stdin.flush()

    def wait_for(self, status, stop=None, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if stop is not None and stop.is_set():
                return False
            try:
                received = self.events.get(timeout=min(.2, max(.01, deadline - time.monotonic())))
            except Empty:
                if self.process.poll() is not None:
                    return False
                continue
            if received == status:
                return True
            if received is None or received == "already_running":
                return False
        return False

    def close(self):
        stop_child(self.process)
        self.reader.join(timeout=5)
        self.process.stdout.close()


def replace_service(root, args, current, expected, stop=None):
    try:
        candidate = ServiceProcess(root, [arg for arg in args if arg != "--open"], prepared=True)
    except OSError:
        return current
    adopted = False
    paused = False
    try:
        if not candidate.wait_for("source_ready", stop) or signature(root) != expected:
            return current
        current.send("pause")
        paused = True
        if not current.wait_for("paused", stop):
            return current
        candidate.send("start")
        if not candidate.wait_for("listening", stop):
            return current
        current.close()
        adopted = True
        return candidate
    except (OSError, BrokenPipeError):
        return current
    finally:
        if not adopted:
            candidate.close()
            if paused and (stop is None or not stop.is_set()) and current.process.poll() is None:
                current.send("resume")
                if not current.wait_for("listening", stop):
                    raise RuntimeError("The previous monitor could not resume after a failed update")


def supervise(root, args, stop=None):
    previous = signature(root)
    pending = None
    pending_at = None
    child = ServiceProcess(root, args)
    try:
        while True:
            if stop is not None and stop.is_set():
                return 0
            code = child.process.poll()
            if code is not None:
                return code
            current = signature(root)
            if current != previous:
                if pending == current and time.monotonic()-pending_at >= 3:
                    built = build_frontend(root, stop=stop)
                    if stop is not None and stop.is_set():
                        return 0
                    if built:
                        previous, pending = current, None
                        print("Frontend build failed; kept the running monitor.", flush=True)
                        continue
                    replacement = replace_service(root, args, child, current, stop)
                    previous, pending = current, None
                    if replacement is child:
                        print("Service update failed; kept the running monitor.", flush=True)
                    else:
                        child = replacement
                        print("Source updated; restarted the local monitor. Keep the browser open.", flush=True)
                elif pending != current:
                    pending = current
                    pending_at = time.monotonic()
            else:
                pending = None
            time.sleep(.5)
    except KeyboardInterrupt:
        return 130
    finally:
        child.close()


def main(args=None):
    args = sys.argv[1:] if args is None else args
    root = Path(__file__).resolve().parents[1]
    if args and args[0] in {"--serve-child", "--prepare-child"}:
        sys.path.insert(0, str(root/"src"))
        from local_activity_monitor.server import main as serve
        if args[0] == "--prepare-child":
            from local_activity_monitor.ui_assets import load_ui_assets
            from local_activity_monitor.frontend_assets import load_frontend_assets
            load_ui_assets()
            load_frontend_assets()
            print(json.dumps({"status": "source_ready"}), flush=True)
            if sys.stdin.readline().strip() != "start":
                return 0
        return serve(args[1:], watch_stdin=True)
    stop = None
    if "--watch-stdin" in args:
        stop = Event()
        def shutdown():
            try:
                sys.stdin.readline()
            finally:
                stop.set()
        Thread(target=shutdown, daemon=True).start()
    prepared = build_frontend(root, stop=stop)
    if stop is not None and stop.is_set():
        return 0
    if prepared:
        return prepared
    if stop is not None:
        return supervise(root, [arg for arg in args if arg != "--watch-stdin"], stop)
    return supervise(root, args)


if __name__ == "__main__":
    with cli_lifetime():
        raise SystemExit(main())
