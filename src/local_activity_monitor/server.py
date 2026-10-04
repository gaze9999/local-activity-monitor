"""Serve a local metadata dashboard using only the Python standard library."""
from __future__ import annotations

import argparse
import base64
import copy
import gzip
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import re
import sys
from pathlib import Path
import tempfile
import threading
import time
import uuid
from urllib.parse import parse_qs, urlsplit
from urllib.request import ProxyHandler, build_opener
import webbrowser

from .collectors import CodexCollector, JevCollector, WINDOWS, monitor_config, name as tool_name, now
from .mcp_records import CATEGORIES, SOURCE, discover_sources, summarize
from .monitor_state import MonitorState
from .error_records import DiagnosticCollector, error_summary, FAILURES


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


def home_id(home):
    return hashlib.sha256(os.path.normcase(str(home.resolve())).encode("utf-8")).hexdigest()[:24]


def existing_instance(port, home):
    if not port:
        return None
    url = f"http://127.0.0.1:{port}/"
    try:
        with build_opener(ProxyHandler({})).open(url+"api/instance", timeout=2) as response:
            value = json.loads(response.read(4097))
        return url if isinstance(value, dict) and value.get("application")=="local-activity-monitor" and value.get("home_id")==home_id(home) else None
    except (OSError, ValueError):
        return None


class Dashboard:
    def __init__(self, home, codex=False, max_files=20, interval=10):
        self.home, self.max_files = home, max_files
        self.jev = JevCollector(home)
        self.codex = CodexCollector(home/"sessions", max_files) if codex else None
        self.interval = interval
        source = Path(__file__).parent
        self.code_revision = hashlib.sha256(b"".join(path.read_bytes() for path in sorted(source.glob("*.py")))).hexdigest()[:12]+"-"+uuid.uuid4().hex[:8]
        self.observations = {"codex": codex, "jev": True, "metadata": True, "git": True, "jev_calls": True, "skills": True, "checks": True, "tool_events": True, "mcp": True, "web": True, "files": True, "errors": True}
        self.mcp_sources, self.mcp_categories, self.tool_descriptions = {}, {}, {}
        self.started_at = now()
        self.monitor = MonitorState()
        self.diagnostics = DiagnosticCollector(home)
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.refresh_lock = threading.RLock()
        self.cache = {}
        self.thread = threading.Thread(target=self.poll, name="metadata-collectors", daemon=True)

    def refresh(self):
        try:
            self._refresh()
        except Exception as error:
            self.monitor.failed(error)
            raise

    def _refresh(self):
        with self.refresh_lock:
            began, cpu = time.perf_counter(), time.process_time()
            before_bytes = self.codex.read_bytes if self.codex else 0
            if self.codex and self.observations["codex"]:
                self.codex.refresh()
            codex = self.codex.snapshot() if self.codex and self.observations["codex"] else {"source": "codex", "health": "disabled", "threads": [], "tools": {}}
            enabled, database, since = monitor_config(self.home)
            if self.observations["codex"] and self.observations["errors"]:
                self.diagnostics.refresh()
            diagnostics = self.diagnostics.snapshot() if self.observations["codex"] and self.observations["errors"] else {"events": [], "health": {"desktop": "disabled", "core": "disabled"}}
            errors = codex.pop("error_events", [])+diagnostics.pop("events")
            sources = discover_sources(self.home)
            mcp = summarize(sources, codex.pop("mcp_events", []), self.mcp_sources, self.mcp_categories)
            availability = {"jev": "jev" in sources or database is not None or any(item["server"] == "jev" for item in mcp["servers"])}
            settings = self.settings()
            assets = Path(__file__).parent/"web"
            revision = self.code_revision+"-"+hashlib.sha256(b"".join((assets/name).read_bytes() for name in ("index.html", "app.js", "style.css"))).hexdigest()[:12]
            cache = {window: {"version": 1, "revision": revision, "label": "本機觀察統計", "started_at": self.started_at, "updated_at": now(), "settings": settings, "availability": availability, "mcp": mcp, "jev": self.jev.snapshot(window) if self.observations["jev"] else {"source": "jev", "health": "paused", "enabled": enabled, "enabled_at": since, "summary": {}, "recent": [], "series": []}, "codex": codex} for window in WINDOWS}
            for snapshot in cache.values():
                jev_errors = []
                if self.observations["errors"]:
                    for event in snapshot["jev"]["recent"]:
                        for index, attempt in enumerate(event.get("attempts", [])):
                            if attempt.get("status") in FAILURES:
                                jev_errors.append({"timestamp": event["timestamp"], "category": "mcp", "source": "jev_telemetry", "severity": "error", "server": "jev", "tool": event["operation"], "code": attempt["status"], "http_status": attempt.get("http_status"), "attempt": index+1})
                snapshot["errors"] = error_summary(errors+jev_errors) | {"diagnostics": diagnostics, "enabled": self.observations["errors"]}
            database_bytes = None
            if database is not None:
                try:
                    database_bytes = database.stat().st_size
                except OSError:
                    pass
            self.monitor.refreshed({"refresh_ms": round((time.perf_counter()-began)*1000, 2), "cpu_ms": round((time.process_time()-cpu)*1000, 2), "read_bytes": (self.codex.read_bytes if self.codex else 0)-before_bytes, "retained_calls": sum(len(state["calls"]) for state in self.codex.files.values()) if self.codex else 0, "buffer_bytes": sum(len(state["buffer"]) for state in self.codex.files.values()) if self.codex else 0, "trimmed_calls": self.codex.trimmed_calls if self.codex else 0, "call_limit": CodexCollector.CALL_LIMIT, "file_limit": CodexCollector.FILE_LIMIT, "buffer_limit": CodexCollector.BUFFER_LIMIT, "jev_database_bytes": database_bytes})
            with self.lock:
                self.cache = cache

    def set_jev_recording(self, enabled):
        with self.refresh_lock:
            configure(self.jev.home, enabled)
            self.monitor.event("recording_enabled" if enabled else "recording_disabled")
            self.refresh()
            current = self.snapshot("24h")["jev"]
            return {"enabled": current["enabled"], "enabled_at": current["enabled_at"]}

    def settings(self):
        return {"interval": self.interval, "max_files": self.max_files, "track_all": self.codex.track_all if self.codex else False, "observations": dict(self.observations), "mcp_sources": dict(self.mcp_sources), "mcp_categories": dict(self.mcp_categories), "tool_descriptions": dict(self.tool_descriptions)}

    def set_settings(self, value):
        allowed = {"interval", "max_files", "track_all", "observations", "mcp_sources", "mcp_categories", "tool_descriptions", "replace_customizations", "recording"}
        if not isinstance(value, dict) or not value or set(value)-allowed:
            raise ValueError()
        observations = value.get("observations", {})
        if "interval" in value and (type(value["interval"]) is not int or not 1 <= value["interval"] <= 3600) or "track_all" in value and type(value["track_all"]) is not bool or not isinstance(observations, dict) or set(observations)-set(self.observations) or any(type(item) is not bool for item in observations.values()):
            raise ValueError()
        if "max_files" in value and (type(value["max_files"]) is not int or not 1 <= value["max_files"] <= 5000):
            raise ValueError()
        if any(key in value and type(value[key]) is not bool for key in ("replace_customizations", "recording")):
            raise ValueError()
        for key in ("mcp_sources", "mcp_categories"):
            items = value.get(key, {})
            if not isinstance(items, dict) or len(items) > 64 or any(not isinstance(name, str) or not SOURCE.fullmatch(name) for name in items):
                raise ValueError()
            if key == "mcp_sources" and any(type(item) is not bool for item in items.values()) or key == "mcp_categories" and any(not isinstance(item, str) or item not in CATEGORIES for item in items.values()):
                raise ValueError()
        descriptions = value.get("tool_descriptions", {})
        if not isinstance(descriptions, dict) or len(descriptions) > 64 or any(not tool_name(key) or not isinstance(text, str) or len(text) > 400 for key, text in descriptions.items()):
            raise ValueError()
        with self.refresh_lock:
            replace = value.get("replace_customizations", False)
            next_descriptions = {key: text for key, text in (descriptions if replace else self.tool_descriptions | descriptions).items() if text.strip()}
            if not replace and any(len(set(existing)|set(value.get(key, {}))) > 64 for key, existing in (("mcp_sources", self.mcp_sources), ("mcp_categories", self.mcp_categories))) or len(next_descriptions) > 64:
                raise ValueError()
            if "recording" in value:
                changed = configure(self.jev.home, value["recording"]) if value["recording"] or monitor_config(self.jev.home)[1] is not None else "unchanged"
                if changed != "unchanged":
                    self.monitor.event("recording_enabled" if value["recording"] else "recording_disabled")
            interval = value.get("interval", self.interval)
            track_all = value.get("track_all", self.codex.track_all if self.codex else False)
            if observations.get("codex") and not self.codex:
                self.codex = CodexCollector(self.home/"sessions", self.max_files)
            self.interval = interval
            self.max_files = value.get("max_files", self.max_files)
            self.observations.update(observations)
            source_changes = value.get("mcp_sources", {})
            next_sources = dict(source_changes) if replace else self.mcp_sources | source_changes
            reenable = any(next_sources.get(name, True) and enabled is False for name, enabled in self.mcp_sources.items())
            self.mcp_sources = next_sources
            self.mcp_categories = dict(value.get("mcp_categories", {})) if replace else self.mcp_categories | value.get("mcp_categories", {})
            self.tool_descriptions = next_descriptions
            if self.codex:
                if reenable or any(enabled and not self.codex.features[key] for key, enabled in observations.items() if key in self.codex.features):
                    self.codex.files.clear()
                    self.codex.next_scan = 0
                self.codex.features.update({key: enabled for key, enabled in self.observations.items() if key in self.codex.features})
                self.codex.mcp_sources = dict(self.mcp_sources)
                if self.codex.max_files != self.max_files:
                    self.codex.max_files = self.max_files
                    self.codex.next_scan = 0
                if self.codex.track_all != track_all:
                    self.codex.track_all = track_all
                    self.codex.next_scan = 0
            self.monitor.event("settings_applied")
            self.refresh()
            return self.settings()

    def jev_detail(self, thread_id, call_id, index):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"] or not self.observations["jev_calls"]:
                return None
            return self.codex.jev_detail(thread_id, call_id, index)

    def skill_detail(self, skill, document=None):
        with self.refresh_lock:
            if not self.codex or not self.observations["codex"] or not self.observations["skills"]:
                return None
            return self.codex.skill_detail(skill, document)

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
            cached = self.cache.get(window)
            result = copy.deepcopy(cached) if cached is not None else {"label": "本機觀察統計", "health": "starting", "started_at": self.started_at, "updated_at": None, "settings": self.settings(), "availability": {}, "mcp": {"servers": [], "events": [], "categories": CATEGORIES}, "codex": {"health": "waiting", "threads": [], "tools": {}}, "jev": {"health": "disabled", "enabled": False, "summary": {}, "recent": [], "series": []}}
        result["monitor"] = self.monitor.snapshot()
        errors = result.setdefault("errors", {"events": [], "diagnostics": {}, "enabled": self.observations["errors"]})
        program_errors = [{"timestamp": event["timestamp"], "category": "monitor", "source": "monitor", "severity": "error", "code": event.get("error_type") or "http_error", "http_status": event.get("http_status"), "reason": event["kind"]} for event in result["monitor"]["events"] if event["kind"] in ("refresh_failed", "http_response_error")]
        errors.update(error_summary(errors["events"]+program_errors))
        return result


def handler(dashboard, port):
    hosts = {f"127.0.0.1:{port}", f"localhost:{port}"}
    assets = {"/": ("index.html", "text/html; charset=utf-8"), "/app.js": ("app.js", "text/javascript; charset=utf-8"), "/style.css": ("style.css", "text/css; charset=utf-8")}

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def setup(self):
            super().setup()
            self.connection.settimeout(15)

        def log_message(self, *args):
            pass

        def reply(self, code, content, content_type="text/plain; charset=utf-8", script_hash=None, style_hash=None):
            plain_bytes = len(content)
            compressed = len(content) >= 1024 and "gzip" in {item.strip() for item in self.headers.get("Accept-Encoding", "").split(",")}
            if compressed:
                content = gzip.compress(content, compresslevel=1)
            snapshot = getattr(self, "path", "").split("?", 1)[0] == "/api/snapshot" and code == 200
            dashboard.monitor.requested(code, plain_bytes if snapshot else None, len(content) if snapshot else None)
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(content)))
            if code >= 400:
                self.close_connection = True
                self.send_header("Connection", "close")
            self.send_header("Vary", "Accept-Encoding")
            if compressed:
                self.send_header("Content-Encoding", "gzip")
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Referrer-Policy", "no-referrer")
            script_policy = " 'sha256-"+script_hash+"'" if script_hash else ""
            style_policy = " 'sha256-"+style_hash+"'" if style_hash else ""
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'"+script_policy+"; style-src 'self'"+style_policy+"; connect-src 'self'; img-src 'self'; frame-ancestors 'none'; base-uri 'none'")
            self.end_headers()
            self.wfile.write(content)

        def local_request(self):
            host, origin = self.headers.get("Host"), self.headers.get("Origin")
            if host not in hosts or origin is not None and origin != "http://"+host:
                self.reply(403, b"Local access only")
                return False
            if self.headers.get("Sec-Fetch-Site") == "cross-site":
                self.reply(403, b"Local access only")
                return False
            return True

        def do_GET(self):
            if not self.local_request():
                return
            url = urlsplit(self.path)
            if url.path == "/api/codex/jev":
                if len(url.query) > 512:
                    self.reply(400, b"Invalid query")
                    return
                query = parse_qs(url.query)
                try:
                    if set(query) != {"thread", "call", "index"} or any(len(item) != 1 for item in query.values()):
                        raise ValueError()
                    thread_id, call_id, index = query["thread"][0], query["call"][0], int(query["index"][0])
                    if not 0 < len(thread_id) <= 160 or not 0 < len(call_id) <= 160 or not 0 <= index < 100:
                        raise ValueError()
                except (ValueError, KeyError):
                    self.reply(400, b"Invalid Jev call")
                    return
                result = dashboard.jev_detail(thread_id, call_id, index)
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/codex/skill":
                query = parse_qs(url.query, keep_blank_values=True)
                document = query.get("file", [None])[0]
                if len(url.query)>2048 or set(query) not in ({"skill"}, {"skill", "file"}) or any(len(value)!=1 for value in query.values()) or not re.fullmatch(r"[^\x00-\x1f\x7f/\\]{1,160}",query["skill"][0]) or query["skill"][0] in (".","..") or document is not None and (not re.fullmatch(r"[^\x00-\x1f\x7f\\:]{1,512}", document) or document.startswith("/") or any(part in ("", ".", "..") for part in document.split("/"))):
                    self.reply(400, b"Invalid skill")
                    return
                result = dashboard.skill_detail(query["skill"][0], document)
                self.reply(200 if result is not None else 409, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/instance":
                self.reply(200, json.dumps({"application": "local-activity-monitor", "home_id": home_id(dashboard.home)}).encode("utf-8"), "application/json; charset=utf-8")
            elif url.path == "/api/snapshot":
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
                root = Path(__file__).parent/"web"
                content, script_hash, style_hash = (root/file).read_bytes(), None, None
                if file == "index.html":
                    # Keep startup assets in one response, with exact CSP hashes.
                    script = (root/"app.js").read_text(encoding="utf-8").encode("utf-8").replace(b"</", b"<\\/")
                    style = (root/"style.css").read_text(encoding="utf-8").encode("utf-8")
                    content = content.replace(b"</body>", b"<script>"+script+b"</script>\n</body>")
                    content = content.replace(b'<link rel="stylesheet" href="/style.css">', b"<style>"+style+b"</style>")
                    script_hash = base64.b64encode(hashlib.sha256(script).digest()).decode("ascii")
                    style_hash = base64.b64encode(hashlib.sha256(style).digest()).decode("ascii")
                self.reply(200, content, mime, script_hash=script_hash, style_hash=style_hash)
            else:
                self.reply(404, b"Not found")

        def do_POST(self):
            if not self.local_request():
                return
            if self.path not in ("/api/jev/recording", "/api/settings"):
                self.reply(405, b"Unsupported operation")
                return
            if self.headers.get("Content-Type", "").split(";", 1)[0].strip() != "application/json" or self.headers.get("Transfer-Encoding"):
                self.reply(400, b"Invalid content type")
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= (262144 if self.path == "/api/settings" else 512):
                    raise ValueError()
                value = json.loads(self.rfile.read(length))
                if self.path == "/api/jev/recording" and (not isinstance(value, dict) or set(value) != {"enabled"} or type(value["enabled"]) is not bool):
                    raise ValueError()
            except (ValueError, TypeError):
                self.reply(400, b"Invalid recording setting")
                return
            try:
                result = dashboard.set_jev_recording(value["enabled"]) if self.path == "/api/jev/recording" else dashboard.set_settings(value)
                code = 200
            except ValueError:
                result, code = {"error": "既有 Jev 設定無效, 請先檢查設定檔"} if self.path == "/api/jev/recording" else {"error": "觀察設定的格式或數值無效"}, 409
            except OSError:
                result, code = {"error": "無法寫入本機 Jev 設定, 請檢查存取權限"}, 500
            self.reply(code, json.dumps(result, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")

    return Handler


def main(argv=None, watch_stdin=False):
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
    if not 0 <= args.port <= 65535 or not 1 <= args.max_files <= 5000:
        parser.error("Invalid port or max-files")
    home = args.codex_home.expanduser().resolve()
    if args.enable_jev or args.disable_jev:
        print(json.dumps({"jev_recording": configure(home, args.enable_jev, args.database), "reload_existing_jev_processes": True}))
    if args.configure_only:
        return 0
    existing = existing_instance(args.port, home)
    if existing:
        print(json.dumps({"status": "already_running", "url": existing}), flush=True)
        if args.open:
            webbrowser.open(existing)
        return 0
    try:
        server = ThreadingHTTPServer(("127.0.0.1", args.port), BaseHTTPRequestHandler)
    except OSError:
        print("Port is already in use. Keep the existing monitor open or choose another --port.", file=sys.stderr)
        return 1
    dashboard = Dashboard(home, args.codex, args.max_files)
    server.RequestHandlerClass = handler(dashboard, server.server_port)
    dashboard.refresh()
    dashboard.thread.start()
    url = f"http://127.0.0.1:{server.server_port}/"
    print(json.dumps({"status": "listening", "url": url, "codex_metadata": args.codex}), flush=True)
    if args.open:
        webbrowser.open(url)
    if watch_stdin:
        def watch_control():
            try:
                if sys.stdin.readline().strip() == "restart":
                    server.shutdown()
            except OSError:
                pass
        threading.Thread(target=watch_control, name="watch-control", daemon=True).start()
    try:
        server.serve_forever(poll_interval=.5)
    except KeyboardInterrupt:
        pass
    finally:
        dashboard.stop.set()
        server.server_close()
        dashboard.thread.join(timeout=5)
    return 0
