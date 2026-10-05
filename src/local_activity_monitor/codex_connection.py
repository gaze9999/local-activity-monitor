"""Project connection evidence from already collected metadata, without probing."""
from datetime import datetime, timezone

RECENT_SECONDS = 300


def connection_status(threads, errors, enabled=True, current=None):
    result = {"state": "unknown" if enabled else "paused", "observed_at": None,
              "last_response_at": None, "evidence": None, "recent_seconds": RECENT_SECONDS}
    if not enabled:
        return result
    current = current or datetime.now(timezone.utc)
    candidates = []
    for thread in threads:
        value = thread.get("token_updated_at")
        if value and not thread.get("metadata_only"):
            candidates.append((value, "response", "model_usage"))
    for event in errors:
        if event.get("category") in ("conversation", "codex") and event.get("source") in ("codex_core", "codex_desktop", "session") and (event.get("reason") == "stream_interrupted" or event.get("code") in ("stream_interrupted", "stream_disconnected", "connection_refused", "network_unavailable") or event.get("cause") in ("connection_refused", "stream_interrupted") or event.get("reason") == "message_submit_failed" and event.get("cause") == "timeout"):
            candidates.append((event.get("timestamp"), "error", "connection_error"))
    valid = []
    for value, state, evidence in candidates:
        try:
            date = datetime.fromisoformat(value.replace("Z", "+00:00"))
            if date.tzinfo and date <= current:
                valid.append((date, value, state, evidence))
        except (ValueError, TypeError, AttributeError):
            continue
    responses = [item for item in valid if item[2] == "response"]
    if responses:
        result["last_response_at"] = max(responses)[1]
    if valid:
        date, value, state, evidence = max(valid, key=lambda item:(item[0], item[2] == "error"))
        result.update(observed_at=value, evidence=evidence)
        if (current-date).total_seconds() <= RECENT_SECONDS:
            result["state"] = state
    return result
