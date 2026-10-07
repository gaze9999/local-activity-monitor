"""Build checkout assets and restart only this child when source files change."""
from pathlib import Path
import subprocess
import sys
import time
from threading import Event, Thread

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from local_activity_monitor.process_lifecycle import cli_lifetime


def signature(root):
    result = []
    paths = list((root/"src/local_activity_monitor").rglob("*.py"))
    paths.extend(path for path in (root/"frontend").glob("*") if path.is_file())
    paths.extend((root / "tools/build_frontend.py", root / "workbench-ui.json"))
    for path in sorted(paths):
        try:
            stat = path.stat()
            result.append((str(path.relative_to(root)), stat.st_mtime_ns, stat.st_size))
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
            child.stdin.close()


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
                    built = subprocess.run([sys.executable, "-I", "-B", str(root / "tools/build_frontend.py")], cwd=root)
                    if built.returncode:
                        previous, pending = current, None
                        print("Frontend build failed; kept the running monitor.", flush=True)
                        continue
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
    prepared = subprocess.run([sys.executable, "-I", "-B", str(root / "tools/build_frontend.py")], cwd=root)
    if prepared.returncode:
        return prepared.returncode
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
    with cli_lifetime():
        raise SystemExit(main())
