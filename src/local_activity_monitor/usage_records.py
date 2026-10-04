"""Project local account allowance snapshots without credentials or billing estimates."""
from datetime import datetime, timezone
import math
import re


def number(value, maximum=2**53):
    return value if type(value) in (int, float) and math.isfinite(value) and 0 <= value <= maximum else None


def allowance(value, when):
    if not isinstance(value, dict) or not when:
        return None
    result = {"updated_at": when, "limits": []}
    for key in ("limit_id", "plan_type"):
        item = value.get(key)
        if isinstance(item, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", item):
            result[key] = item
    for key in ("primary", "secondary"):
        item = value.get(key)
        if not isinstance(item, dict):
            continue
        used, minutes, reset = number(item.get("used_percent"), 100), number(item.get("window_minutes"), 525600), number(item.get("resets_at"), 253402300799)
        window = {"id": key}
        if used is not None:
            window.update(used_percent=used, remaining_percent=100-used)
        if minutes is not None and minutes > 0:
            window["window_minutes"] = minutes
        if reset is not None:
            try:
                window["resets_at"] = datetime.fromtimestamp(reset, timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
            except (ValueError, OverflowError, OSError):
                pass
        if len(window) > 1:
            result["limits"].append(window)
    credits = value.get("credits")
    if isinstance(credits, dict):
        clean = {key: credits[key] for key in ("has_credits", "unlimited") if type(credits.get(key)) is bool}
        balance = credits.get("balance")
        if isinstance(balance, str) and re.fullmatch(r"\d{1,16}(?:\.\d{1,16})?", balance):
            balance = float(balance)
        balance = number(balance)
        if balance is not None:
            clean["balance"] = balance
        if clean:
            result["credits"] = clean
    return result if result["limits"] or result.get("credits") else None
