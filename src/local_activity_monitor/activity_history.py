"""Retain bounded activity metadata without request or response bodies."""
from datetime import datetime, timedelta, timezone
import json
import os

from .error_records import identifier
from .mcp_records import reference_url, response_metadata, resource_ids, SOURCE
from .sqlite_records import database_path


def sql_identity(event):
    return tuple(event.get(key) for key in ("source", "thread_id", "call_id", "index", "file", "record_id", "record_offset", "record_hash", "timestamp", "statement"))


def web_identity(event):
    return tuple(event.get(key) for key in ("thread_id", "call_id", "index", "timestamp", "tool"))


def mcp_identity(event):
    return (event.get('server'),)+web_identity(event)


def merge_event(previous, current):
    result = dict(previous)
    for key, value in current.items():
        if value is not None:
            if isinstance(value, dict):
                result[key] = merge_event(result.get(key) if isinstance(result.get(key), dict) else {}, value)
            elif isinstance(value, list) and isinstance(result.get(key), list):
                combined = list(result[key])
                combined.extend(item for item in value if item not in combined)
                result[key] = combined[:30]
            else:
                result[key] = value
    return result


class ActivityHistory:
    DEFAULT_DAYS = 7
    SQL_LIMIT, WEB_LIMIT, MCP_LIMIT, BYTE_LIMIT = 500, 1000, 1000, 1024*1024
    STATEMENTS = {"SELECT", "EXPLAIN", "INSERT", "UPDATE", "DELETE", "REPLACE", "CREATE", "ALTER", "DROP", "PRAGMA", "VACUUM", "ANALYZE", "REINDEX", "BEGIN", "COMMIT", "ROLLBACK", "SAVEPOINT", "RELEASE", "ATTACH", "DETACH", "DATABASE_EVENT"}

    def __init__(self, path):
        self.path, self.sql, self.web, self.mcp, self.saved = path, [], [], [], None
        self.retention_days = self.DEFAULT_DAYS
        self.health = "ok"
        try:
            if path.is_symlink():
                raise OSError()
            if path.exists():
                with path.open('rb') as stream:
                    raw = stream.read(self.BYTE_LIMIT+1)
                if len(raw)>self.BYTE_LIMIT:
                    raise ValueError()
                value = json.loads(raw)
                if not isinstance(value, dict) or value.get('version') not in (1, 2, 3) or not isinstance(value.get('sql'), list) or not isinstance(value.get('web'), list) or not isinstance(value.get('mcp', []), list):
                    raise ValueError()
                days = value.get('retention_days', self.DEFAULT_DAYS)
                if type(days) is not int or not 1 <= days <= 365:
                    raise ValueError()
                self.retention_days = days
                self.sql = self.project(value['sql'][:self.SQL_LIMIT], 'sql')
                self.web = self.project(value['web'][:self.WEB_LIMIT], 'web')
                self.mcp = self.project(value.get('mcp', [])[:self.MCP_LIMIT], 'mcp')
        except (OSError, ValueError):
            self.health = "unavailable"

    def project(self, events, kind):
        boundary, ceiling = datetime.now(timezone.utc)-timedelta(days=self.retention_days), datetime.now(timezone.utc)+timedelta(minutes=1)
        result = {}
        for value in events:
            if not isinstance(value, dict) or kind=='sql' and value.get('statement') not in self.STATEMENTS or kind=='web' and value.get('server')!='web' or kind=='mcp' and (not isinstance(value.get('server'), str) or not SOURCE.fullmatch(value['server']) or value['server']=='web'):
                continue
            try:
                date = datetime.fromisoformat(value['timestamp'].replace('Z', '+00:00'))
                if not date.tzinfo or not boundary<=date<=ceiling:
                    continue
            except (KeyError, ValueError, TypeError, AttributeError):
                continue
            event = {'timestamp': value['timestamp']}
            keys = ('source', 'operation', 'engine', 'recognition', 'result', 'tool', 'thread_id', 'call_id', 'file', 'record_hash') if kind=='sql' else ('tool', 'thread_id', 'call_id', 'action', 'category', 'server')
            for key in keys:
                item = identifier(value.get(key))
                if item is not None:
                    event[key] = item
            for key in ('index', 'record_id', 'record_offset', 'rows_affected', 'rows_returned'):
                item = value.get(key)
                if type(item) is int and 0<=item<=2**63-1:
                    event[key] = item
            for key in ('duration_ms', 'container_duration_ms'):
                item = value.get(key)
                if type(item) in (int, float) and 0<=item<=86400000:
                    event[key] = item
            if kind=='sql':
                if not {'operation', 'engine', 'recognition'}<=event.keys():
                    continue
                event['statement'] = value['statement']
                for key in ('database', 'workdir'):
                    path = database_path(value.get(key))
                    if path:
                        event[key] = path
                identity = sql_identity(event)
            else:
                event['nested'] = value.get('nested') is True
                if isinstance(value.get('completed_at'), str) and len(value['completed_at'])<=40:
                    event['completed_at'] = value['completed_at']
                for key in ('metadata', 'result'):
                    original = value.get(key, {})
                    original = original if isinstance(original, dict) else {}
                    event[key] = {'references': list(dict.fromkeys(url for item in (original.get('references', []) if isinstance(original.get('references'), list) else [])[:30] if (url:=reference_url(item))))[:30]} if kind=='web' else response_metadata(event['server'], original) if key=='result' else {'resources': resource_ids(original.get('resources', {}) if isinstance(original.get('resources'), dict) else {})}
                    if kind=='web' and key=='metadata':
                        event[key]['operations'] = [item for item in (original.get('operations', []) if isinstance(original.get('operations'), list) else [])[:20] if item in ('search_query', 'open', 'click', 'find', 'screenshot', 'image_query', 'finance', 'weather', 'sports', 'time')]
                    elif key=='result' and identifier(original.get('status')):
                        event[key]['status'] = identifier(original['status'])
                identity = web_identity(event) if kind=='web' else mcp_identity(event)
            result[identity] = merge_event(result.get(identity, {}), event)
        return sorted(result.values(), key=lambda event:event['timestamp'], reverse=True)[:self.SQL_LIMIT if kind=='sql' else self.WEB_LIMIT if kind=='web' else self.MCP_LIMIT]

    def update(self, sql, web, mcp=()):
        self.sql = self.project(self.sql+sql, 'sql')
        self.web = self.project(self.web+web, 'web')
        self.mcp = self.project(self.mcp+list(mcp), 'mcp')
        raw = self.encode()
        while len(raw)>self.BYTE_LIMIT and (self.sql or self.web or self.mcp):
            entries = min((items for items in (self.sql, self.web, self.mcp) if items), key=lambda items:items[-1]['timestamp'])
            entries.pop()
            raw = self.encode()
        if raw==self.saved:
            return
        temporary = self.path.with_name(self.path.name+'.tmp')
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            if self.path.is_symlink() or temporary.is_symlink():
                raise OSError()
            with temporary.open('wb') as stream:
                stream.write(raw)
            if os.name!='nt':
                temporary.chmod(0o600)
            os.replace(temporary, self.path)
            self.saved, self.health = raw, 'ok'
        except OSError:
            self.health = 'unavailable'

    def encode(self):
        return json.dumps({'version': 3, 'retention_days': self.retention_days, 'sql': self.sql, 'web': self.web, 'mcp': self.mcp}, ensure_ascii=True, separators=(',', ':')).encode()

    def snapshot(self, kind):
        return self.project(self.sql if kind=='sql' else self.web if kind=='web' else self.mcp, kind)
