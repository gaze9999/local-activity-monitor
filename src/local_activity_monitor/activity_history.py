"""Retain bounded activity metadata without request or response bodies."""
from datetime import datetime, timedelta, timezone
import json
import sqlite3

from .error_records import identifier
from .mcp_records import reference_url, response_metadata, resource_ids, SOURCE
from .sqlite_records import database_path
from .history_store import HistoryStore


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
    DEFAULT_DAYS = 90
    SQL_LIMIT, WEB_LIMIT, MCP_LIMIT, BYTE_LIMIT = 500, 1000, 1000, 1024*1024
    STATEMENTS = {"CONNECT", "CLOSE", "SELECT", "EXPLAIN", "INSERT", "UPDATE", "DELETE", "REPLACE", "CREATE", "ALTER", "DROP", "PRAGMA", "VACUUM", "ANALYZE", "REINDEX", "BEGIN", "COMMIT", "ROLLBACK", "SAVEPOINT", "RELEASE", "ATTACH", "DETACH", "DATABASE_EVENT"}

    def __init__(self, path):
        self.store = HistoryStore(path, "activity")
        self.path, self.sql, self.web, self.mcp, self.saved = self.store.path, [], [], [], None
        self.retention_days = self.DEFAULT_DAYS
        self.health = "ok"
        self.last_incoming = None
        self.last_prune_day = None
        try:
            raw = self.store.load(self.BYTE_LIMIT)
            if raw is not None:
                if len(raw)>self.BYTE_LIMIT:
                    raise ValueError()
                value = json.loads(raw)
                if not isinstance(value, dict) or value.get('version') not in (1, 2, 3) or not isinstance(value.get('sql'), list) or not isinstance(value.get('web'), list) or not isinstance(value.get('mcp', []), list):
                    raise ValueError()
                days = value.get('retention_days', self.DEFAULT_DAYS)
                if type(days) is not int or not 0 <= days <= 3650:
                    raise ValueError()
                self.retention_days = days
                self.sql = self.project(value['sql'][:self.SQL_LIMIT], 'sql')
                self.web = self.project(value['web'][:self.WEB_LIMIT], 'web')
                self.mcp = self.project(value.get('mcp', [])[:self.MCP_LIMIT], 'mcp')
                if self.store.imported:
                    self.store.save(self.encode())
        except (OSError, ValueError, sqlite3.Error):
            self.health = "unavailable"

    def project(self, events, kind, bounded=True):
        boundary = datetime.now(timezone.utc)-timedelta(days=self.retention_days) if self.retention_days else datetime.min.replace(tzinfo=timezone.utc)
        ceiling = datetime.now(timezone.utc)+timedelta(minutes=1)
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
        ordered = sorted(result.values(), key=lambda event:event['timestamp'], reverse=True)
        return ordered[:self.SQL_LIMIT if kind=='sql' else self.WEB_LIMIT if kind=='web' else self.MCP_LIMIT] if bounded else ordered

    def update(self, sql, web, mcp=()):
        # Validate all incoming metadata before trimming the in-memory view.
        incoming = {'version': 3, 'retention_days': self.retention_days,
                    'sql': self.project(list(sql), 'sql', False),
                    'web': self.project(list(web), 'web', False),
                    'mcp': self.project(list(mcp), 'mcp', False)}
        incoming_raw = json.dumps(incoming, ensure_ascii=True, separators=(',', ':')).encode()
        current = datetime.now(timezone.utc)
        day = (current.date(), self.retention_days)
        boundary = current-timedelta(days=self.retention_days) if self.retention_days else datetime.min.replace(tzinfo=timezone.utc)
        expired = any(rows and datetime.fromisoformat(rows[-1]['timestamp'].replace('Z','+00:00'))<boundary for rows in (self.sql,self.web,self.mcp,incoming['sql'],incoming['web'],incoming['mcp']))
        if incoming_raw == self.last_incoming and self.last_prune_day == day and not expired and self.health == 'ok':
            return
        storage_error = None
        try:
            self.store.save(incoming_raw)
            if self.last_prune_day != day or expired:
                removed = self.store.prune(self.retention_days)
                self.last_prune_day = day if removed < 500 else None
        except (OSError, sqlite3.Error) as error:
            self.health = 'unavailable'
            storage_error = error
        for kind,limit,identity in (('sql',self.SQL_LIMIT,sql_identity),('web',self.WEB_LIMIT,web_identity),('mcp',self.MCP_LIMIT,mcp_identity)):
            previous = getattr(self,kind)
            # Existing rows already crossed the metadata boundary. Revalidate new
            # rows once, then merge stable identities and apply time/count bounds.
            merged = {identity(event):event for event in previous}
            for event in incoming[kind]:
                key = identity(event)
                merged[key] = merge_event(merged.get(key,{}),event)
            ordered = sorted((event for event in merged.values() if datetime.fromisoformat(event['timestamp'].replace('Z','+00:00'))>=boundary),key=lambda event:event['timestamp'],reverse=True)
            setattr(self,kind,ordered[:limit])
        raw = self.encode()
        while len(raw)>self.BYTE_LIMIT and (self.sql or self.web or self.mcp):
            entries = min((items for items in (self.sql, self.web, self.mcp) if items), key=lambda items:items[-1]['timestamp'])
            entries.pop()
            raw = self.encode()
        if storage_error is not None:
            return
        if raw==self.saved:
            self.last_incoming = incoming_raw if len(incoming_raw)<=self.BYTE_LIMIT else None
            return
        try:
            self.store.save(raw)
            self.saved, self.health = raw, 'ok'
            self.last_incoming = incoming_raw if len(incoming_raw)<=self.BYTE_LIMIT else None
        except (OSError, sqlite3.Error):
            self.health = 'unavailable'

    def encode(self):
        return json.dumps({'version': 3, 'retention_days': self.retention_days, 'sql': self.sql, 'web': self.web, 'mcp': self.mcp}, ensure_ascii=True, separators=(',', ':')).encode()

    def snapshot(self, kind):
        return self.project(self.sql if kind=='sql' else self.web if kind=='web' else self.mcp, kind)
