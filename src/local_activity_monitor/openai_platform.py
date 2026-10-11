"""Read-only OpenAI Platform metadata, credentials stay in the service environment."""

import copy
from datetime import datetime, timedelta, timezone
from decimal import Decimal, InvalidOperation
import json
import os
import re
import threading
import time
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, build_opener

from .codex_metadata import date
from .public_api import NoRedirect

SOURCES = {"models": ("/models", "OPENAI_API_KEY"),
           "usage": ("/organization/usage/completions", "OPENAI_ADMIN_KEY"),
           "costs": ("/organization/costs", "OPENAI_ADMIN_KEY")}
FIELDS = ("input_tokens", "input_cached_tokens", "output_tokens", "num_model_requests")


class OpenAIPlatformSource:
    CACHE_SECONDS = 60
    TIMEOUT = 5
    MAX_BYTES = 1024*1024

    def __init__(self):
        self.cache = {}
        self.locks = {key: threading.Lock() for key in SOURCES}

    def query(self, source):
        if source not in SOURCES:
            raise ValueError("Unknown Platform source")
        with self.locks[source]:
            cached = self.cache.get(source)
            if cached and time.monotonic()-cached[0] < self.CACHE_SECONDS:
                return copy.deepcopy(cached[1])
            stamp = datetime.now(timezone.utc)
            result = {"source": source, "health": "missing_credentials", "items": [], "checked_at": stamp.isoformat()}
            path, variable = SOURCES[source]
            key = os.environ.get(variable)
            if key and len(key) <= 4096 and not any(ord(char) < 33 or ord(char) > 126 for char in key):
                query = {}
                if source != "models":
                    start = int((stamp.replace(hour=0, minute=0, second=0, microsecond=0)-timedelta(days=6)).timestamp())
                    end = int(stamp.timestamp())
                    query = {"start_time": start, "end_time": end, "bucket_width": "1d", "limit": 7}
                    result.update(start_time=date(start), end_time=date(end))
                request = Request("https://api.openai.com/v1"+path+("?"+urlencode(query) if query else ""),
                                  headers={"Authorization": "Bearer "+key, "Accept": "application/json"})
                try:
                    with build_opener(NoRedirect()).open(request, timeout=self.TIMEOUT) as response:
                        raw = response.read(self.MAX_BYTES+1)
                    if len(raw) > self.MAX_BYTES:
                        raise ValueError("Platform response too large")
                    result.update(self._project(source, json.loads(raw, parse_float=Decimal)), health="ok")
                except HTTPError as error:
                    result["health"] = "unauthorized" if error.code in (401, 403) else "rate_limited" if error.code == 429 else "error"
                except (OSError, ValueError, InvalidOperation, RecursionError):
                    result["health"] = "error"
            self.cache[source] = time.monotonic(), result
            return copy.deepcopy(result)

    @staticmethod
    def _project(source, value):
        rows = value.get("data") if isinstance(value, dict) else None
        if not isinstance(rows, list) or len(rows) > (500 if source == "models" else 7):
            raise ValueError("Invalid Platform records")
        items = []
        if source == "models":
            for row in rows:
                if not isinstance(row, dict) or not isinstance(row.get("id"), str) or not re.fullmatch(r"[A-Za-z0-9_.:/\-]{1,200}", row["id"]):
                    raise ValueError("Invalid model metadata")
                owner = row.get("owned_by")
                items.append({"id": row["id"], "created_at": date(row.get("created")),
                              "owned_by": owner if isinstance(owner, str) and re.fullmatch(r"[A-Za-z0-9_.:/\-]{1,160}", owner) else None})
            return {"items": items}
        if type(value.get("has_more")) is not bool:
            raise ValueError("Invalid Platform page")
        for row in rows:
            if not isinstance(row, dict) or not isinstance(row.get("results"), list) or len(row["results"]) > 1000:
                raise ValueError("Invalid Platform bucket")
            start, end = date(row.get("start_time")), date(row.get("end_time"))
            if start is None or end is None or start >= end:
                raise ValueError("Invalid Platform bucket time")
            entries = row["results"]
            item = {"start_time": start, "end_time": end}
            if source == "usage":
                for field in FIELDS:
                    numbers = [entry.get(field) if isinstance(entry, dict) else None for entry in entries]
                    item[field] = sum(numbers) if numbers and all(type(number) is int and 0 <= number <= 2**63-1 for number in numbers) else None
            else:
                amounts = {}
                for entry in entries:
                    amount = entry.get("amount") if isinstance(entry, dict) else None
                    if not isinstance(amount, dict) or type(amount.get("value")) not in (int, Decimal) or not isinstance(amount.get("currency"), str) or not re.fullmatch(r"[a-z]{3}", amount["currency"]):
                        raise ValueError("Invalid Platform cost")
                    number = Decimal(amount["value"])
                    if not number.is_finite() or abs(number) > Decimal("1e12") or number.as_tuple().exponent < -18:
                        raise ValueError("Invalid Platform amount")
                    currency = amount["currency"]
                    amounts[currency] = amounts.get(currency, Decimal(0))+number
                item["amounts"] = {key: format(number, "f") for key, number in amounts.items()}
            items.append(item)
        return {"items": items, "has_more": value["has_more"]}
