"""Read bounded schedule metadata without prompts, account IDs or run messages."""
from contextlib import closing
from pathlib import Path
import re
import sqlite3

from .codex_metadata import date, text

SCHEDULE_LIMIT = 500
RUN_LIMIT = 1000
FIELDS = ("id", "name", "status", "kind", "next_run_at", "last_run_at", "rrule",
          "model", "reasoning_effort", "project_id", "target_thread_id",
          "execution_environment", "created_at", "updated_at")


def timestamp(value):
    return date(value/1000 if type(value) in (int, float) and value > 100_000_000_000 else value)


def read_schedules(home: Path, source_info=None):
    path = home/"sqlite/codex-dev.db"
    result = {"items": [], "health": "missing", "row_limit": SCHEDULE_LIMIT, "run_limit": RUN_LIMIT}
    report = {"name": "automations", "location": str(path), "health": "missing", "fields": [],
              "row_limit": SCHEDULE_LIMIT, "run_limit": RUN_LIMIT, "rows_read": 0, "runs_read": 0}
    try:
        with closing(sqlite3.connect(path.as_uri()+"?mode=ro", uri=True, timeout=.08)) as db:
            db.execute("PRAGMA query_only=ON")
            columns = {row[1] for row in db.execute("PRAGMA table_info(automations)")}
            if not {"id", "status"} <= columns:
                result["health"] = report["health"] = "unsupported"
            else:
                wanted = [field for field in FIELDS if field in columns]
                report["fields"] = ["automations."+field for field in wanted]
                order = "next_run_at IS NULL,next_run_at" if "next_run_at" in columns else "id"
                for row in db.execute("SELECT "+",".join(wanted)+f" FROM automations ORDER BY {order} LIMIT {SCHEDULE_LIMIT}"):
                    report["rows_read"] += 1
                    values = dict(zip(wanted, row))
                    identity = values.get("id")
                    if not isinstance(identity, str) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,160}", identity):
                        continue
                    item = {"id": identity, "name": text(values.get("name"), 256)}
                    for field in ("status", "kind", "model", "reasoning_effort", "project_id", "target_thread_id", "execution_environment"):
                        value = values.get(field)
                        if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_.:-]{1,160}", value):
                            item[field] = value
                    for field in ("next_run_at", "last_run_at", "created_at", "updated_at"):
                        item[field] = timestamp(values.get(field))
                    rule = values.get("rrule")
                    if isinstance(rule, str) and len(rule) <= 2048 and re.fullmatch(r"[A-Za-z0-9=;,:+_./\-\r\n ]+", rule):
                        item["rrule"] = rule
                    result["items"].append(item)
                result["health"] = report["health"] = "ok"
                runs = {row[1] for row in db.execute("PRAGMA table_info(automation_runs)")}
                if {"automation_id", "thread_id", "created_at"} <= runs:
                    lookup = {item["id"]: item for item in result["items"]}
                    run_fields = [field for field in ("automation_id", "thread_id", "created_at", "status") if field in runs]
                    report["fields"].extend("automation_runs."+field for field in run_fields)
                    for row in db.execute("SELECT "+",".join(run_fields)+f" FROM automation_runs ORDER BY created_at DESC LIMIT {RUN_LIMIT}"):
                        report["runs_read"] += 1
                        values = dict(zip(run_fields, row))
                        item = lookup.get(values["automation_id"])
                        identity = values["thread_id"]
                        if item is None or "last_thread_id" in item or not isinstance(identity, str) or not re.fullmatch(r"[A-Za-z0-9-]{1,160}", identity):
                            continue
                        item["last_thread_id"] = identity
                        item["last_run_at"] = item.get("last_run_at") or timestamp(values["created_at"])
                        status = values.get("status")
                        if isinstance(status, str) and re.fullmatch(r"[A-Za-z_]{1,40}", status):
                            item["last_run_status"] = status
    except (OSError, sqlite3.Error):
        result["health"] = report["health"] = "unavailable" if path.is_file() else "missing"
    if source_info is not None:
        source_info[str(path)+"#automations"] = report
    return result
