"""Resume selected session metadata without storing unparsed source bytes."""
from contextlib import closing
import hashlib
import json
import re
import sqlite3

from .history_store import HistoryStore


FIELDS = ('thread_id', 'model', 'reasoning_effort', 'execution', 'context_time',
          'tokens', 'token_time', 'token_priority', 'created_at', 'updated_at',
          'status', 'activity_type', 'trigger', 'task_start',
          'task_intervals', 'errors', 'task_time', 'cwd', 'allowance', 'discard', 'agent_incoming')
CALL_FIELDS = ('tool', 'timestamp', 'completed_at', 'error', 'nested_tools',
               'git', 'skills', 'checks', 'jev', 'mcp', 'isolated', 'files',
               'sqlite', 'agent_messages', 'request_offset', 'response_offset')


class SessionCheckpoint:
    BYTE_LIMIT = 16*1024*1024

    def __init__(self, root):
        self.root = root
        self.store = HistoryStore(root.parent/'monitoring/thread-state.json', 'threads')
        self.health = 'missing'
        self.bytes_read = 0

    def key(self, path):
        return hashlib.sha256(str(path.relative_to(self.root)).encode()).hexdigest()

    def signature(self, path, offset):
        info = path.stat()
        if offset>info.st_size:
            return None
        # Detect replacement and in-place rewrites at both ends of the consumed
        # prefix. Neither sampled bytes nor unfinished source lines are saved.
        with path.open('rb') as stream:
            head = stream.read(min(offset, 256))
            stream.seek(max(0, offset-256))
            tail = stream.read(min(offset, 256))
        self.bytes_read += len(head)+len(tail)
        return json.dumps([info.st_dev, info.st_ino, hashlib.sha256(head+tail).hexdigest()])

    def load(self, path, call_limit=8192):
        if not self.store.path.exists():
            return None
        try:
            with closing(self.store.connect()) as db:
                row = db.execute('SELECT offset, signature, payload FROM session_cursors WHERE source_key=?', (self.key(path),)).fetchone()
                calls = db.execute('SELECT call_id,payload FROM session_calls WHERE source_key=? ORDER BY event_time DESC,call_id LIMIT ?', (self.key(path),max(0,min(8192,call_limit)))).fetchall()
            if row is None:
                return None
            offset, signature, payload = row
            if type(offset) is not int or offset<0 or not isinstance(payload,str) or len(payload)>self.BYTE_LIMIT or self.signature(path,offset)!=signature:
                return None
            value = json.loads(payload)
            if not isinstance(value,dict) or value.get('version')!=1 or not isinstance(value.get('state'),dict):
                return None
            state = value['state']
            if set(state)-set(FIELDS):
                return None
            state['calls'] = {}
            if not isinstance(state.get('thread_id'),str) or not re.fullmatch(r'[0-9a-fA-F-]{36}',state['thread_id']) or state.get('status') not in ('observed','running','completed'):
                return None
            call_bytes = 0
            for identity, raw in calls:
                if not isinstance(raw,str) or (call_bytes:=call_bytes+len(raw))>self.BYTE_LIMIT:
                    return None
                state['calls'][identity] = json.loads(raw)
            for identity, call in state['calls'].items():
                if not isinstance(identity,str) or not 1<=len(identity)<=160 or not isinstance(call,dict) or set(call)-set(CALL_FIELDS) or not isinstance(call.get('tool'),str) or not isinstance(call.get('timestamp'),(str,type(None))):
                    return None
                for key in ('git','skills','checks','jev','mcp','files','sqlite'):
                    if not isinstance(call.get(key,[]),list) or any(not isinstance(item,dict) for item in call.get(key,[])):
                        return None
            for key in ('execution','tokens','task_intervals'):
                if not isinstance(state.get(key),dict):
                    return None
            if not isinstance(state.get('errors'),list) or any(not isinstance(item,dict) for item in state['errors']):
                return None
            self.health = 'ok'
            return state|{'offset':offset,'buffer':b'', 'history_cursor':None, 'partial_history':offset<path.stat().st_size}
        except (OSError,sqlite3.Error,ValueError,TypeError,RecursionError):
            self.health = 'unavailable'
            return None

    def save(self, path, state):
        if state['offset'] is None:
            return False
        offset = state['offset']-len(state['buffer'])
        projected = {key:state[key] for key in FIELDS if key in state}
        try:
            payload = json.dumps({'version':1,'state':projected},ensure_ascii=True,separators=(',',':'),allow_nan=False)
            if len(payload)>self.BYTE_LIMIT:
                self.health = 'oversized'
                return False
            signature = self.signature(path,offset)
            if signature is None:
                return False
            with closing(self.store.connect(write=True)) as db, db:
                db.execute('INSERT INTO session_cursors VALUES(?,?,?,?) ON CONFLICT(source_key) DO UPDATE SET offset=excluded.offset, signature=excluded.signature, payload=excluded.payload WHERE session_cursors.offset!=excluded.offset OR session_cursors.signature!=excluded.signature OR session_cursors.payload!=excluded.payload', (self.key(path),offset,signature,payload))
                calls = [(self.key(path),identity,call.get('timestamp'),json.dumps({key:call[key] for key in CALL_FIELDS if key in call},ensure_ascii=True,separators=(',',':'),allow_nan=False)) for identity in state.get('dirty_calls',()) if (call:=state['calls'].get(identity)) is not None]
                db.executemany('INSERT INTO session_calls VALUES(?,?,?,?) ON CONFLICT(source_key,call_id) DO UPDATE SET event_time=excluded.event_time,payload=excluded.payload WHERE session_calls.payload!=excluded.payload',calls)
                db.executemany('DELETE FROM session_calls WHERE source_key=? AND call_id=?', [(self.key(path),identity) for identity in state.get('retired_ids',())])
            state['dirty_calls'],state['retired_ids'] = set(),set()
            self.health = 'ok'
            return True
        except (OSError,sqlite3.Error,ValueError,TypeError,RecursionError):
            self.health = 'unavailable'
            return False
