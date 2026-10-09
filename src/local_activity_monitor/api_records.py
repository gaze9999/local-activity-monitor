"""Project model API diagnostics to bounded metadata, never request bodies."""
import json
import math
import re
from collections import Counter


API_TARGETS = ("codex_api::", "codex_otel.log_only", "codex_core::responses_retry")
TOKEN = re.compile(r'(?<![\w.])([\w.]{1,64})=("(?:\\.|[^"\\])*(?:"|$)|[^\s{},:"]+)|"(?:\\.|[^"\\])*(?:"|$)')


def api_diagnostic(module, body):
    if not isinstance(module, str) or not module.startswith(API_TARGETS) or not isinstance(body, str):
        return None
    fields = {}
    for match in TOKEN.finditer(body[:8192]):
        key, value = match.groups()
        if key is None:
            continue
        if value.startswith('"'):
            try:
                value = json.loads(value)
            except ValueError:
                continue
        fields[key] = value
    # Remove quoted fields and strings before matching fixed diagnostic headings.
    heading = TOKEN.sub("", body[:8192])
    event = fields.get("event.name")
    operation, kind, status = None, None, "unknown"
    if event in ("codex.api_request", "codex.websocket.request"):
        operation, kind = event, "request"
        if fields.get("success") == "true":
            status = "sent" if event == "codex.websocket.request" else "returned"
        elif fields.get("success") == "false":
            status = "failed"
    elif module == "codex_core::responses_retry" and "retries" in fields:
        operation, kind, status = "stream_retry", "retry", "retrying"
    elif module == "codex_api::endpoint::responses_websocket":
        for label, result in (("successfully connected to websocket", "connected"), ("failed to connect to websocket", "failed"), ("connecting to websocket", "connecting")):
            if label in heading:
                operation, kind, status = "websocket_connection", "connection", result
                break
    if kind is None:
        return None
    endpoint = fields.get("endpoint", fields.get("api.path"))
    if kind == "request" and endpoint is not None and endpoint not in ("/responses", "responses", "/v1/responses", "/chat/completions", "/v1/chat/completions"):
        return None
    result = {"event_kind": kind, "operation": operation, "status": status}
    aliases = {"model": ("model",), "provider": ("provider",), "request_id": ("request_id", "request.id"), "trace_id": ("trace_id", "trace.id"), "turn_id": ("turn_id", "turn.id"), "thread_id": ("thread_id", "thread.id", "conversation.id"), "transport": ("transport",), "endpoint": ("endpoint", "api.path")}
    for key, choices in aliases.items():
        value = next((fields[name] for name in choices if name in fields), None)
        pattern = r"/[a-zA-Z0-9_/-]{1,80}" if key == "endpoint" else r"[a-zA-Z0-9_.:-]{1,160}"
        if isinstance(value, str) and re.fullmatch(pattern, value) and not value.startswith(("sk-", "sk_")):
            result[key] = value
    if "thread_id" in result and not re.fullmatch(r"[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}", result["thread_id"]):
        result.pop("thread_id")
    for key in ("duration_ms", "input_tokens", "output_tokens", "total_tokens", "retries", "attempt"):
        value = fields.get(key)
        if isinstance(value, str) and re.fullmatch(r"\d{1,16}(?:\.\d{1,6})?", value):
            number = float(value) if "." in value else int(value)
            if math.isfinite(number) and 0 <= number <= 2**53-1 and (key == "duration_ms" or type(number) is int):
                result[key] = number
    for key in ("http_status", "status_code", "http.status_code"):
        value = fields.get(key)
        if isinstance(value, str) and re.fullmatch(r"[1-5]\d{2}", value):
            result["http_status"] = int(value)
            if kind == "request":
                result["status"] = "failed" if int(value) >= 400 else "sent" if event == "codex.websocket.request" else "returned"
            break
    if kind != "request":
        for key in ("input_tokens", "output_tokens", "total_tokens", "duration_ms"):
            result.pop(key, None)
    return result


def api_summary(events):
    events = sorted({event["observation_id"]: event for event in events}.values(), key=lambda event: event["timestamp"], reverse=True)
    counts = Counter(event["event_kind"] for event in events)
    calls = [event for event in events if event["event_kind"] == "request"]
    return {"events": events, "requests": len(calls), "connections": counts["connection"], "retry_events": counts["retry"], "failed_requests": sum(event["status"] == "failed" for event in calls), "limit": 1000}
