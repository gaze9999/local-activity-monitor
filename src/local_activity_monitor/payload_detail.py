"""Preserve bounded structure when presenting on-demand tool content."""
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

from .operation_records import redact


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
    return None


def complete_payload(value):
    """Project a selected payload for paged display; never persist its contents."""
    def project(item, depth=0):
        if depth > 64:
            return '[內容過深]'
        if isinstance(item, str):
            parsed = parse_text(item, full=True)
            return project(parsed['value'], depth+1) if parsed else redact(item, limit=None)
        if isinstance(item, dict):
            result = {}
            for key, child in item.items():
                label = str(key)
                result[label] = '[已隱藏]' if redact({label: None}, limit=None).get(label) == '[已隱藏]' else project(child, depth+1)
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
    digest = hashlib.sha256(text.encode('utf-8')).hexdigest()
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
            clean = redact(item)
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
                protected = redact({label: None})[label] == "[已隱藏]"
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
