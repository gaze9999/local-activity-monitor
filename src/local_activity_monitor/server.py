"""Serve a local metadata dashboard using only the Python standard library."""
from __future__ import annotations

import argparse
import copy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import uuid
from urllib.parse import parse_qs, urlsplit
import webbrowser

from .collectors import CodexCollector, JevCollector, WINDOWS, monitor_config, now


def data_root():
    if os.name == "nt":
        value = Path(os.environ.get("LOCALAPPDATA", ""))
        return (value if value.is_absolute() else Path.home()/"AppData/Local") / "local-activity-monitor"
    if os.sys.platform == "darwin":
        return Path.home()/"Library/Application Support/local-activity-monitor"
    value = Path(os.environ.get("XDG_DATA_HOME", ""))
    return (value if value.is_absolute() else Path.home()/".local/share") / "local-activity-monitor"


def configure(home, enabled, database=None):
    path = home / "monitoring/jev-monitor.json"
    before = path.read_bytes() if path.exists() else None
    old_enabled, old_database, since = monitor_config(home)
    if before is not None and old_database is None:
        raise ValueError("Existing monitor config is invalid; inspect it before replacement")
    database = (database or old_database or data_root()/"state/jev.sqlite3").expanduser()
    if not database.is_absolute():
        raise ValueError("Database path must be absolute")
    value = {"version": 1, "enabled": enabled, "database": str(database), "enabled_at": since if old_database == database and since else now()}
    after = (json.dumps(value, indent=2)+"\n").encode("utf-8")
    if before is not None and json.loads(before) == value:
        return "unchanged"
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if before is not None:
        backup = path.with_name(path.name+"."+uuid.uuid4().hex[:8]+".bak")
        with backup.open("xb") as stream:
            stream.write(before)
        if os.name != "nt":
            backup.chmod(0o600)
    descriptor, temporary = tempfile.mkstemp(dir=path.parent, prefix=".monitor-")
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(after)
            stream.flush()
            os.fsync(stream.fileno())
        current = path.read_bytes() if path.exists() else None
        if current != before:
            raise ValueError("Monitor config changed during update")
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    return "enabled" if enabled else "disabled"


class Dashboard:
    def __init__(self, home, codex=False, max_files=20, interval=4):
        self.jev = JevCollector(home)
        self.codex = CodexCollector(home/"sessions", max_files) if codex else None
        self.interval = interval
        self.started_at = now()
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.cache = {}
        self.thread = threading.Thread(target=self.poll, name="metadata-collectors", daemon=True)

    def refresh(self):
        if self.codex:
            self.codex.refresh()
        codex = self.codex.snapshot() if self.codex else {"source": "codex", "health": "disabled", "threads": [], "tools": {}}
        cache = {window: {"version": 1, "label": "本機觀察統計", "started_at": self.started_at, "updated_at": now(), "jev": self.jev.snapshot(window), "codex": codex} for window in WINDOWS}
        with self.lock:
            self.cache = cache

    def poll(self):
        while not self.stop.is_set():
            try:
                self.refresh()
            except Exception:
                # Collector errors cannot expose record content or exception paths
                pass
            self.stop.wait(self.interval)

    def snapshot(self, window):
        with self.lock:
            return copy.deepcopy(self.cache.get(window, {"label": "本機觀察統計", "health": "starting"}))


def handler(dashboard, port):
    hosts = {f"127.0.0.1:{port}", f"localhost:{port}"}
    assets = {"/": ("index.html", "text/html; charset=utf-8"), "/app.js": ("app.js", "text/javascript; charset=utf-8"), "/style.css": ("style.css", "text/css; charset=utf-8")}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, code, content, content_type="text/plain; charset=utf-8"):
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(content)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Referrer-Policy", "no-referrer")
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; frame-ancestors 'none'; base-uri 'none'")
            self.end_headers()
            self.wfile.write(content)

        def do_GET(self):
            host, origin = self.headers.get("Host"), self.headers.get("Origin")
            if host not in hosts or origin is not None and origin != "http://"+host:
                self.reply(403, b"Local access only")
                return
            if self.headers.get("Sec-Fetch-Site") == "cross-site":
                self.reply(403, b"Local access only")
                return
            url = urlsplit(self.path)
            if url.path == "/api/snapshot":
                if len(url.query) > 128:
                    self.reply(400, b"Invalid query")
                    return
                query = parse_qs(url.query)
                window = query.get("window", ["24h"])[0]
                if window not in WINDOWS or set(query)-{"window"}:
                    self.reply(400, b"Invalid window")
                    return
                raw = json.dumps(dashboard.snapshot(window), ensure_ascii=False, allow_nan=False).encode("utf-8")
                self.reply(200, raw, "application/json; charset=utf-8")
            elif url.path in assets:
                file, mime = assets[url.path]
                self.reply(200, (Path(__file__).parent/"web"/file).read_bytes(), mime)
            else:
                self.reply(404, b"Not found")

        def do_POST(self):
            self.reply(405, b"Read-only dashboard")

    return Handler


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex-home", type=Path, default=Path(os.environ.get("CODEX_HOME") or Path.home()/".codex"))
    parser.add_argument("--port", type=int, default=8787)
    parser.add_argument("--codex", action="store_true", help="Opt in to read recent Codex session metadata")
    parser.add_argument("--max-files", type=int, default=20)
    parser.add_argument("--open", action="store_true")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--enable-jev", action="store_true", help="Opt in to persistent Jev metadata recording")
    group.add_argument("--disable-jev", action="store_true")
    parser.add_argument("--database", type=Path)
    parser.add_argument("--configure-only", action="store_true")
    args = parser.parse_args(argv)
    if not 0 <= args.port <= 65535 or not 1 <= args.max_files <= 100:
        parser.error("Invalid port or max-files")
    home = args.codex_home.expanduser().resolve()
    if args.enable_jev or args.disable_jev:
        print(json.dumps({"jev_recording": configure(home, args.enable_jev, args.database), "reload_existing_jev_processes": True}))
    if args.configure_only:
        return 0
    dashboard = Dashboard(home, args.codex, args.max_files)
    server = ThreadingHTTPServer(("127.0.0.1", args.port), BaseHTTPRequestHandler)
    server.RequestHandlerClass = handler(dashboard, server.server_port)
    dashboard.refresh()
    dashboard.thread.start()
    url = f"http://127.0.0.1:{server.server_port}/"
    print(json.dumps({"status": "listening", "url": url, "codex_metadata": args.codex}), flush=True)
    if args.open:
        webbrowser.open(url)
    try:
        server.serve_forever(poll_interval=.5)
    except KeyboardInterrupt:
        pass
    finally:
        dashboard.stop.set()
        server.server_close()
        dashboard.thread.join(timeout=5)
    return 0
