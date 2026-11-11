"""Frame bounded session chunks while keeping byte offsets source-relative."""
from importlib import import_module


LINE_LIMIT = 1024 * 1024


def _positioned(lines, start, limit):
    for line in lines:
        if line and len(line) <= limit:
            yield start, line
        start += len(line) + 1


def _reverse(lines, end, limit):
    for line in reversed(lines):
        end -= len(line) + 1
        if line and len(line) <= limit:
            yield end, line


def _forward(data, discard, limit):
    lines = data.split(b"\n")
    tail = lines.pop()
    skipped = 0
    if discard and lines:
        skipped = len(lines.pop(0)) + 1
        discard = False
    if len(tail) > limit:
        tail, discard = b"", True
    return skipped, tail, discard, _positioned(lines, skipped, limit)


def _backfill(data, skip_prefix, limit, reverse):
    lines = data.split(b"\n")
    start = len(lines.pop(0)) + 1 if skip_prefix else 0
    rows = _reverse(lines, len(data) + 1, limit) if reverse else _positioned(lines, start, limit)
    return start, rows


try:
    _native = import_module("._session_scan", __package__)
except ModuleNotFoundError as error:
    if error.name != f"{__package__}._session_scan":
        raise
    _native = None

forward = _native.forward if _native is not None else _forward
backfill = _native.backfill if _native is not None else _backfill
