"""Filter already retained metadata without reading any additional source."""
from datetime import datetime, timedelta, timezone

HOURS = {"1h": 1, "24h": 24, "7d": 168, "all": None}


def cutoff(window, reference=None):
    hours = HOURS[window]
    return (reference or datetime.now(timezone.utc))-timedelta(hours=hours) if hours is not None else None


def contains(event, boundary):
    if boundary is None:
        return True
    value = event.get("timestamp")
    if not isinstance(value, str):
        return False
    try:
        when = datetime.fromisoformat(value.replace("Z", "+00:00"))
        return bool(when.tzinfo) and when >= boundary
    except ValueError:
        return False
