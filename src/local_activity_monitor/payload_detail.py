"""Preserve bounded structure when presenting on-demand tool content."""
import ast
from contextlib import contextmanager
from contextvars import ContextVar
import csv
import hashlib
from datetime import date, time
import io
import json
import re
import xml.etree.ElementTree as ET

try:
    import tomllib
except ImportError:
    tomllib = None

from .operation_records import literal, redact

payload_masking = ContextVar("payload_masking", default=True)


@contextmanager
def mask_payloads(enabled=True):
    """Apply one request's display masking without changing metadata collection."""
    if type(enabled) is not bool:
        raise ValueError("Invalid payload masking")
    token = payload_masking.set(enabled)
    try:
        yield
    finally:
        payload_masking.reset(token)


def visible_context_record(record):
    """Select user/assistant messages and explicit public summaries, never raw reasoning."""
    if not isinstance(record, dict) or record.get("type") != "response_item" or record.get("channel") == "analysis":
        return None
    item = record.get("payload")
    if not isinstance(item, dict) or item.get("channel") == "analysis":
        return None
    if item.get("type") == "reasoning":
        summary = item.get("summary")
        if not isinstance(summary, list):
            return None
        summary = [{"type": "summary_text", "text": child["text"]} for child in summary if isinstance(child, dict) and child.get("type") == "summary_text" and isinstance(child.get("text"), str)]
        return {"type": "reasoning", "summary": summary} if summary else None
    if item.get("type") != "message" or item.get("role") not in ("user", "assistant"):
        return None
    content = item.get("content")
    if not isinstance(content, list):
        return None
    content = [{"type": child["type"], "text": child["text"]} for child in content if isinstance(child, dict) and child.get("type") in ("input_text", "output_text", "text") and isinstance(child.get("text"), str)]
    if not content:
        return None
    return {"type": "message", "role": item["role"], "channel": item.get("channel"), "content": content}


def parse_literal(text):
    """Read data literals only, with bounded size, node count and depth."""
    if len(text) > 1024*1024:
        return None
    try:
        tree = ast.parse(text, mode='eval')
        if sum(1 for _ in ast.walk(tree)) <= 20000:
            value = ast.literal_eval(tree)

            def normalize(item, depth=0):
                if depth > 64:
                    raise ValueError()
                if isinstance(item, dict):
                    if any(not isinstance(key, (str, int, float, bool)) and key is not None for key in item):
                        raise ValueError()
                    return {str(key): normalize(child, depth+1) for key, child in item.items()}
                if isinstance(item, (list, tuple)):
                    return [normalize(child, depth+1) for child in item]
                if item is None or isinstance(item, (str, int, float, bool)):
                    return item
                raise ValueError()

            if isinstance(value, (dict, list, tuple)):
                return {'format': 'Python literal', 'value': normalize(value)}
    except (ValueError, SyntaxError, TypeError, RecursionError, OverflowError):
        pass
    try:
        value, end = literal(text)
        if end == len(text) and isinstance(value, (dict, list)):
            return {'format': 'Object literal', 'value': value}
    except (ValueError, RecursionError):
        pass
    return None


def parse_yaml(text):
    """Read an indentation-only YAML subset without tags, anchors or aliases."""
    if len(text) > 1024*1024:
        return None
    lines = []
    for raw in text.splitlines():
        if not raw.strip() or raw.lstrip().startswith('#'):
            continue
        if '\t' in raw[:len(raw)-len(raw.lstrip())]:
            return None
        lines.append((len(raw)-len(raw.lstrip(' ')), raw.lstrip(' ')))
    if not lines or len(lines) > 20000:
        return None
    index = 0

    def scalar(value):
        value = value.strip()
        if value.startswith(('!', '&', '*', '|', '>')):
            raise ValueError()
        if value.startswith(('"', "'", '{', '[')):
            parsed = parse_text(value)
            if parsed:
                return parsed['value']
            if value.startswith("'") and value.endswith("'"):
                return value[1:-1].replace("''", "'")
            if value.startswith('"'):
                return json.loads(value)
            raise ValueError()
        value = re.split(r'\s+#', value, maxsplit=1)[0].rstrip()
        if value in ('null', 'Null', 'NULL', '~'):
            return None
        if value.lower() in ('true', 'false'):
            return value.lower() == 'true'
        if re.fullmatch(r'-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?', value):
            return json.loads(value)
        if ': ' in value or value.endswith(':'):
            raise ValueError()
        return value

    def pair(body):
        match = re.match(r'([^:{}\[\],]+):(?:\s+(.*)|$)', body)
        if not match:
            raise ValueError()
        key = scalar(match[1])
        if not isinstance(key, (str, int, float, bool)) or str(key) == '<<':
            raise ValueError()
        return str(key), match[2] or ''

    def block(indent, depth=0):
        nonlocal index
        if depth > 12:
            raise ValueError()
        sequence = lines[index][1].startswith('- ') or lines[index][1] == '-'
        result = [] if sequence else {}
        while index < len(lines) and lines[index][0] == indent:
            body = lines[index][1]
            if sequence:
                if body != '-' and not body.startswith('- '):
                    raise ValueError()
                body = body[1:].strip()
                index += 1
                if not body:
                    child = block(lines[index][0], depth+1) if index < len(lines) and lines[index][0] > indent else None
                elif re.match(r'[^:{}\[\],]+:(?:\s|$)', body):
                    key, value = pair(body)
                    child = {key: scalar(value) if value else block(lines[index][0], depth+1) if index < len(lines) and lines[index][0] > indent+2 else None}
                    if index < len(lines) and lines[index][0] == indent+2:
                        extra = block(indent+2, depth+1)
                        if not isinstance(extra, dict) or child.keys() & extra.keys():
                            raise ValueError()
                        child.update(extra)
                else:
                    child = scalar(body)
                result.append(child)
            else:
                key, value = pair(body)
                if key in result:
                    raise ValueError()
                index += 1
                if value in ('|', '>', '|-', '>-', '|+', '>+'):
                    # Multiline scalars require whitespace preservation beyond this subset.
                    raise ValueError()
                result[key] = scalar(value) if value else block(lines[index][0], depth+1) if index < len(lines) and lines[index][0] > indent else None
            if index < len(lines) and lines[index][0] > indent:
                raise ValueError()
        return result
    try:
        result = block(lines[0][0])
        return {'format': 'YAML subset', 'value': result} if index == len(lines) else None
    except (ValueError, RecursionError, TypeError):
        return None


def parse_text(text, format=None, full=False):
    """Parse retrieved data without evaluation, external entities or a character cap."""
    if not isinstance(text, str):
        return None
    text = text.strip()
    try:
        if text.startswith(('[', '{')):
            try:
                value = json.loads(text)
                if isinstance(value, (dict, list)):
                    return {'format': 'JSON', 'value': value}
            except ValueError:
                pass
        if text.startswith(('{', '[', '(')):
            parsed = parse_literal(text)
            if parsed:
                return parsed
        lines = text.splitlines()
        if 1<len(lines) and all(line.lstrip().startswith(('{', '[')) for line in lines if line.strip()):
            return {'format': 'JSONL', 'value': [json.loads(line) for line in lines if line.strip()]}
        if text.startswith('<') and '<!doctype' not in text.lower() and '<!entity' not in text.lower():
            root = ET.fromstring(text)
            count = 0

            def element(node, depth=0):
                nonlocal count
                count += 1
                if depth>(64 if full else 8) or not full and count>200:
                    return '[內容截斷]'
                result = {}
                if node.attrib:
                    result['attributes'] = node.attrib
                if node.text and node.text.strip():
                    result['text'] = node.text.strip()
                if len(node):
                    result['children'] = [element(child, depth+1) for child in (node if full else list(node)[:100])]
                return {node.tag: result}

            return {'format': 'XML', 'value': element(root)}
        if tomllib and (format=='toml' or re.match(r'(?:\[[^\n]+\]|[A-Za-z_]\w*\s*=)', text)):
            value = tomllib.loads(text)
            if value:
                return {'format': 'TOML', 'value': value}
        if len(lines)>1 and (format in ('csv', 'tsv') or ',' in lines[0] or '\t' in lines[0]):
            dialect = csv.Sniffer().sniff(text[:4096], delimiters=',\t')
            rows = list(csv.reader(io.StringIO(text), dialect))
            if 1<len(rows) and 1<len(rows[0])<=100 and all(len(row)==len(rows[0]) for row in rows):
                headers = rows[0]
                value = {'columns': headers, 'rows': [dict(zip(headers, row)) for row in rows[1:]]} if len(set(headers))==len(headers) else rows
                return {'format': 'TSV' if dialect.delimiter=='\t' else 'CSV', 'value': value}
    except (ValueError, RecursionError, ET.ParseError, csv.Error, TypeError):
        pass
    if format in ('yaml', 'yml') or len(text.splitlines()) > 1 and re.match(r'(?:[^:{}\[\],]+:\s|[^:{}\[\],]+:$|- )', text.splitlines()[0]):
        return parse_yaml(text)
    return None


def complete_payload(value):
    """Project a selected payload for paged display; never persist its contents."""
    def project(item, depth=0):
        if depth > 64:
            return '[內容過深]'
        if isinstance(item, str):
            parsed = parse_text(item, full=True)
            return project(parsed['value'], depth+1) if parsed else redact(item, limit=None) if payload_masking.get() else item
        if isinstance(item, dict):
            result = {}
            for key, child in item.items():
                label = str(key)
                result[label] = '[已隱藏]' if payload_masking.get() and redact({label: None}, limit=None).get(label) == '[已隱藏]' else project(child, depth+1)
            return result
        if isinstance(item, list):
            return [project(child, depth+1) for child in item]
        if isinstance(item, (date, time)):
            return item.isoformat()
        return redact(item)
    return {'value': project(value), 'truncated': False}


CONTENT_FIELDS = {'request', 'response', 'output', 'sql', 'text'}


def content_page(value, offset=0, revision=None):
    """Page already masked display text, checking that all pages share one version."""
    text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False)
    mode = b"masked\0" if payload_masking.get() else b"unmasked\0"
    digest = hashlib.sha256(mode+text.encode('utf-8')).hexdigest()
    if revision is not None and revision != digest or offset < 0 or offset > len(text):
        raise ValueError('Content changed or invalid offset')
    end = min(offset+32768, len(text))
    return {'content_page': True, 'text': text[offset:end], 'format': 'text' if isinstance(value, str) else 'json',
            'total': len(text), 'next': end if end < len(text) else None, 'revision': digest}


def paged_content(result, field=None, offset=0, revision=None):
    if field is not None:
        if field not in CONTENT_FIELDS or field not in result:
            raise KeyError('Invalid content field')
        return content_page(result[field], offset, revision)
    return {key: content_page(value) if key in CONTENT_FIELDS and value is not None else value for key, value in result.items()}


def bounded_payload(value, limit=32768):
    remaining, truncated, nodes = limit, False, 0

    def project(item, depth=0):
        nonlocal remaining, truncated, nodes
        nodes += 1
        if nodes > 200 or depth > 10 or remaining <= 0:
            truncated = True
            return "[內容截斷]"
        if isinstance(item, str):
            parsed = parse_text(item)
            if parsed:
                return project(parsed['value'], depth+1)
            clean = redact(item) if payload_masking.get() else item[:32768]
            truncated |= len(item) > len(clean) and len(item) > 32768 or len(clean) > remaining
            clean = clean[:remaining]
            remaining -= len(clean)
            return clean
        if isinstance(item, dict):
            result = {}
            entries = list(item.items())
            truncated |= len(entries) > 100
            for key, child in entries[:100]:
                if remaining <= 0 or nodes >= 200:
                    truncated = True
                    break
                label = str(key)[:160]
                remaining -= len(label)
                # Preserve the existing credential field whitelist before recursion.
                protected = payload_masking.get() and redact({label: None})[label] == "[已隱藏]"
                result[label] = "[已隱藏]" if protected else project(child, depth+1)
            return result
        if isinstance(item, list):
            result = []
            truncated |= len(item) > 100
            for child in item[:100]:
                if remaining <= 0 or nodes >= 200:
                    truncated = True
                    break
                result.append(project(child, depth+1))
            return result
        if isinstance(item, (date, time)):
            return item.isoformat()
        return redact(item)

    return {"value": project(value), "truncated": truncated}
