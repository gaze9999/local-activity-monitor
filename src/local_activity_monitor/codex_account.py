"""Opt-in official Codex app-server account metadata, with bounded stdio reads.

Protocol: https://learn.chatgpt.com/docs/app-server (2026-10-06).
Only initialize/initialized and read-only account methods are emitted.
"""
from __future__ import annotations

import copy
import datetime as dt
import json
import math
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import threading
import time

from . import __version__

_METHODS = ("account/read", "account/rateLimits/read", "account/usage/read")
_SUMMARY = ("lifetimeTokens", "peakDailyTokens", "longestRunningTurnSec", "currentStreakDays", "longestStreakDays")
_MAX_OUTPUT = 1024 * 1024
_TIMEOUT = 10.0
_CACHE_SECONDS = 60.0


def _number(value, *, integer=False):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    if value < 0 or value > 2**63 - 1 or not math.isfinite(value):
        return None
    if integer and not isinstance(value, int):
        return None
    return value


def _label(value):
    return value if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9 _./:-]{1,120}", value) else None


def _credits(value):
    if not isinstance(value, dict):
        return None
    result = {}
    for key, output in (("hasCredits", "has_credits"), ("unlimited", "unlimited")):
        if isinstance(value.get(key), bool):
            result[output] = value[key]
    balance = value.get("balance")
    if balance is None or isinstance(balance, str) and re.fullmatch(r"-?\d{1,20}(?:\.\d{1,12})?", balance):
        result["balance"] = balance
    return result or None


def _window(value):
    if not isinstance(value, dict):
        return None
    used = _number(value.get("usedPercent"), integer=True)
    result = {"used_percent": used, "remaining_percent": max(0, 100 - used) if used is not None else None}
    for key, output in (("windowDurationMins", "window_minutes"), ("resetsAt", "resets_at")):
        if key in value:
            number = _number(value[key], integer=True)
            if key == "resetsAt":
                try:
                    number = dt.datetime.fromtimestamp(number, dt.timezone.utc).isoformat() if number is not None else None
                except (ValueError, OSError, OverflowError):
                    number = None
            result[output] = number
    return result


def _limits(value):
    mapped = value.get("rateLimitsByLimitId")
    if isinstance(mapped, dict):
        entries = list(mapped.items())[:32]
    else:
        legacy = value.get("rateLimits")
        entries = [(_label(legacy.get("limitId")) or "codex", legacy)] if isinstance(legacy, dict) else []
    result = {}
    for name, bucket in entries:
        name = _label(name)
        if name is None or not isinstance(bucket, dict):
            continue
        item = {"limit_id": _label(bucket.get("limitId")) or name}
        for key, output in (("limitName", "limit_name"), ("planType", "plan_type"), ("rateLimitReachedType", "reached_type")):
            if key in bucket:
                item[output] = _label(bucket[key])
        for key in ("primary", "secondary"):
            if key in bucket:
                item[key] = _window(bucket[key])
        if "credits" in bucket:
            item["credits"] = _credits(bucket["credits"])
        result[name] = item
    return result


def _usage(value):
    summary = value.get("summary")
    cleaned = {key: _number(summary.get(key), integer=True) for key in _SUMMARY} if isinstance(summary, dict) else None
    buckets = value.get("dailyUsageBuckets")
    daily = None
    if isinstance(buckets, list):
        daily = []
        for row in buckets[:366]:
            if not isinstance(row, dict):
                continue
            date = row.get("startDate")
            tokens = _number(row.get("tokens"), integer=True)
            try:
                if not isinstance(date, str) or dt.date.fromisoformat(date).isoformat() != date or tokens is None:
                    continue
            except ValueError:
                continue
            daily.append({"start_date": date, "tokens": tokens})
    return {"summary": cleaned, "daily_usage_buckets": daily}


def _executable():
    if os.name == "nt":
        found = shutil.which("codex.exe")
        if found and Path(found).suffix.lower() == ".exe":
            return found
        local = os.environ.get("LOCALAPPDATA")
        if local:
            candidate = Path(local) / "Programs" / "OpenAI" / "Codex" / "bin" / "codex.exe"
            if candidate.is_file():
                return str(candidate)
        return None
    return shutil.which("codex")


class _ReadFailure(Exception):
    pass


class _RpcError(Exception):
    def __init__(self, code):
        self.code = code


class CodexAccountSource:
    """Cached per selected CODEX_HOME; disabled calls never launch a process."""

    def __init__(self, home):
        self.home = Path(home)
        self._cache = None
        self._cached_at = None
        self._lock = threading.Lock()

    @staticmethod
    def _empty(status):
        return {"health": status, "updated_at": None, "limits": {}, "limit_id": None, "credits": None,
                "plan_type": None, "account_usage": None,
                "methods": {method: {"health": status} for method in _METHODS}}

    def snapshot(self, enabled=False):
        if enabled is not True:
            return self._empty("disabled")
        with self._lock:
            now = time.monotonic()
            if self._cache is not None and self._cached_at is not None and now - self._cached_at < _CACHE_SECONDS:
                return copy.deepcopy(self._cache)
            value = self._read()
            self._cache = value
            self._cached_at = time.monotonic()
            return copy.deepcopy(value)

    def _read(self):
        result = self._empty("unavailable")
        executable = _executable()
        if executable is None:
            return result
        process = None
        reader = None
        stop = threading.Event()
        inbox = queue.Queue(maxsize=128)
        deadline = time.monotonic() + _TIMEOUT

        def read_output():
            total = 0
            try:
                while not stop.is_set():
                    raw = process.stdout.readline(_MAX_OUTPUT - total + 1)
                    if not raw:
                        inbox.put_nowait(_ReadFailure("closed"))
                        return
                    total += len(raw)
                    if total > _MAX_OUTPUT:
                        inbox.put_nowait(_ReadFailure("output_limit"))
                        return
                    message = json.loads(raw)
                    if not isinstance(message, dict):
                        raise _ReadFailure("invalid_response")
                    # Notifications carry unrelated data and are discarded immediately.
                    if "id" in message and "method" not in message:
                        inbox.put_nowait(message)
            except (OSError, ValueError, RecursionError, queue.Full, _ReadFailure):
                try:
                    inbox.put_nowait(_ReadFailure("invalid_response"))
                except queue.Full:
                    pass

        def send(method, params, request_id=None):
            payload = {"method": method, "params": params}
            if request_id is not None:
                payload["id"] = request_id
            process.stdin.write(json.dumps(payload, separators=(",", ":")).encode("utf-8") + b"\n")
            process.stdin.flush()

        def request(method, params, request_id):
            if time.monotonic() >= deadline:
                raise _ReadFailure("timeout")
            send(method, params, request_id)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise _ReadFailure("timeout")
                try:
                    message = inbox.get(timeout=remaining)
                except queue.Empty as exc:
                    raise _ReadFailure("timeout") from exc
                if isinstance(message, Exception):
                    raise message
                if type(message.get("id")) is not int or message["id"] != request_id:
                    continue
                if "error" in message:
                    error = message["error"]
                    raise _RpcError(error.get("code") if isinstance(error, dict) else None)
                value = message.get("result")
                if not isinstance(value, dict):
                    raise _ReadFailure("invalid_response")
                return value

        try:
            env = dict(os.environ)
            env["CODEX_HOME"] = str(self.home)
            process = subprocess.Popen(
                [executable, "app-server", "--listen", "stdio://", "-c", "analytics.enabled=false"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                env=env, shell=False, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            reader = threading.Thread(target=read_output, name="codex-account-reader", daemon=True)
            reader.start()
            request("initialize", {"clientInfo": {"name": "local_activity_monitor", "title": "Local Activity Monitor", "version": __version__}}, 0)
            send("initialized", {})
            for index, method in enumerate(_METHODS, 1):
                try:
                    payload = request(method, {"refreshToken": False} if method == "account/read" else {}, index)
                    if method == "account/read":
                        account = payload.get("account")
                        result["plan_type"] = _label(account.get("planType")) if isinstance(account, dict) else None
                    elif method == "account/rateLimits/read":
                        result["limits"] = _limits(payload)
                        preferred = result["limits"].get("codex") or next(iter(result["limits"].values()), {})
                        result["limit_id"] = preferred.get("limit_id")
                        result["credits"] = preferred.get("credits")
                        if result["plan_type"] is None:
                            result["plan_type"] = preferred.get("plan_type")
                    else:
                        result["account_usage"] = _usage(payload)
                    result["methods"][method] = {"health": "ok"}
                except _RpcError as exc:
                    result["methods"][method] = {"health": "unavailable" if exc.code == -32601 else "error"}
                except (OSError, _ReadFailure):
                    result["methods"][method] = {"health": "error"}
                    # A timed out or broken pipe cannot safely service later reads.
                    break
            statuses = [item["health"] for item in result["methods"].values()]
            result["health"] = "ok" if all(status == "ok" for status in statuses) else "partial" if "ok" in statuses else "error"
            result["updated_at"] = dt.datetime.now(dt.timezone.utc).isoformat()
        except (OSError, _ReadFailure, _RpcError):
            result["health"] = "error"
            result["methods"] = {method: {"health": "error"} for method in _METHODS}
        finally:
            stop.set()
            if process is not None:
                # End only the child created above; never target the desktop process.
                if process.poll() is None:
                    try:
                        process.terminate()
                        process.wait(timeout=0.5)
                    except (OSError, subprocess.TimeoutExpired):
                        try:
                            process.kill()
                            process.wait(timeout=0.5)
                        except (OSError, subprocess.TimeoutExpired):
                            pass
                for stream in (process.stdin, process.stdout):
                    if stream is not None:
                        try:
                            stream.close()
                        except OSError:
                            pass
            if reader is not None:
                reader.join(timeout=0.5)
        return result
