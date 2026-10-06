"""Run checkout source and restart only this child when Python files change."""
from pathlib import Path
import subprocess
import sys
import time
from threading import Event, Thread


def signature(root):
    result = []
    for path in sorted((root/"src/local_activity_monitor").rglob("*.py")):
        try:
            stat = path.stat()
            result.append((str(path.relative_to(root)), stat.st_mtime_ns, stat.st_size))
        except OSError:
            continue
    return tuple(result)


def stop_child(child):
    if child.poll() is not None:
        return
    try:
        if child.stdin:
            child.stdin.write("restart\n")
            child.stdin.flush()
        else:
            child.terminate()
    except (OSError, BrokenPipeError):
        child.terminate()
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=5)


def supervise(root, args, stop=None):
    previous = signature(root)
    pending = None
    pending_at = None
    child = subprocess.Popen([sys.executable, "-I", "-B", str(root/"tools/watch.py"), "--serve-child", *args], cwd=root, stdin=subprocess.PIPE, text=True)
    try:
        while True:
            if stop is not None and stop.is_set():
                return 0
            code = child.poll()
            if code is not None:
                return code
            current = signature(root)
            if current != previous:
                if pending == current and time.monotonic()-pending_at >= 3:
                    stop_child(child)
                    child = subprocess.Popen([sys.executable, "-I", "-B", str(root/"tools/watch.py"), "--serve-child", *[arg for arg in args if arg != "--open"]], cwd=root, stdin=subprocess.PIPE, text=True)
                    previous, pending = current, None
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
        stop_child(child)


def main(args=None):
    args = sys.argv[1:] if args is None else args
    root = Path(__file__).resolve().parents[1]
    if args and args[0] == "--serve-child":
        sys.path.insert(0, str(root/"src"))
        from local_activity_monitor.server import main as serve
        return serve(args[1:], watch_stdin=True)
    if "--watch-stdin" in args:
        stop = Event()
        def shutdown():
            try:
                sys.stdin.readline()
            finally:
                stop.set()
        Thread(target=shutdown, daemon=True).start()
        return supervise(root, [arg for arg in args if arg != "--watch-stdin"], stop)
    return supervise(root, args)


if __name__ == "__main__":
    raise SystemExit(main())
