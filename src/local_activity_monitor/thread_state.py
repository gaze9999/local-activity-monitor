"""Retain confirmed lifecycle checkpoints across collector restarts."""
from datetime import datetime
import json
import sqlite3
import re
import time
from .history_store import HistoryStore


class ThreadState:
    LIMIT = 1000
    SKILL_LIMIT = 500
    BYTE_LIMIT = 512*1024

    def __init__(self, path):
        self.store = HistoryStore(path, "threads")
        self.path, self.entries = self.store.path, {}
        self.skills = []
        self.saved, self.next_save = None, 0
        self.load_health, self.write_health = "missing", None
        try:
            raw = self.store.load(self.BYTE_LIMIT)
            if raw is not None:
                if len(raw) > self.BYTE_LIMIT:
                    self.load_health = "oversized"
                    return
                value = json.loads(raw)
                if isinstance(value, dict) and value.get('version') == 1 and isinstance(value.get('entries'), dict):
                    for key, entry in list(value['entries'].items())[:self.LIMIT]:
                        clean = self.project(key, entry)
                        if clean:
                            self.entries[key] = clean
                    if isinstance(value.get('skills'), list):
                        self.skills = [clean for entry in value['skills'][:self.SKILL_LIMIT] if (clean := self.project_skill(entry))]
                    self.load_health = "ok"
                    if self.store.imported:
                        self.store.save(json.dumps({'version':1, 'entries':self.entries, 'skills':self.skills}, separators=(',', ':')).encode())
                else:
                    self.load_health = "unsupported"
        except (OSError, sqlite3.Error):
            self.load_health = "unavailable"
        except (ValueError, TypeError, AttributeError, RecursionError):
            self.load_health = "unsupported"

    def project(self, key, entry):
        if not isinstance(key, str) or not re.fullmatch(r'rollout-[A-Za-z0-9_.-]{1,180}\.jsonl', key) or not isinstance(entry, dict):
            return None
        if entry.get('status') not in ('running', 'completed') or type(entry.get('offset')) is not int or not 0 <= entry['offset'] <= 2**63-1:
            return None
        identity = entry.get('thread_id')
        if not isinstance(identity, str) or not re.fullmatch(r'[a-fA-F0-9-]{36}', identity):
            return None
        clean = {'thread_id': identity, 'status': entry['status'], 'offset': entry['offset']}
        for key in ('task_time', 'task_start'):
            value = entry.get(key)
            if value is None and key == 'task_start':
                clean[key] = None
                continue
            if not isinstance(value, str) or len(value) > 40:
                return None
            try:
                stamp = datetime.fromisoformat(value.replace('Z', '+00:00'))
                if not stamp.tzinfo:
                    return None
            except ValueError:
                return None
            clean[key] = value
        return clean

    def project_skill(self, entry):
        if not isinstance(entry, dict):
            return None
        fields = {'thread_id': r'[a-fA-F0-9-]{36}', 'call_id': r'[A-Za-z0-9_.:-]{1,160}', 'skill': r'[A-Za-z0-9_. \u3400-\u9fff-]{1,160}'}
        clean = {}
        for key, pattern in fields.items():
            value = entry.get(key)
            if not isinstance(value, str) or not re.fullmatch(pattern, value):
                return None
            clean[key] = value
        value = entry.get('timestamp')
        try:
            if not isinstance(value, str) or len(value) > 40 or not datetime.fromisoformat(value.replace('Z', '+00:00')).tzinfo:
                return None
        except ValueError:
            return None
        clean.update(timestamp=value, has_document=False)
        return clean

    def update(self, states):
        lifecycle_changed = False
        skills = {(entry['thread_id'], entry['call_id'], entry['skill']):entry for entry in self.skills}
        for state in states:
            for identity, call in state.get('calls', {}).items():
                for item in call.get('skills', []):
                    entry = self.project_skill({'thread_id':state['thread_id'], 'call_id':identity, 'skill':item.get('skill'), 'timestamp':call.get('timestamp')})
                    if entry:
                        skills[(entry['thread_id'], entry['call_id'], entry['skill'])] = entry
            if not state.get('task_time'):
                continue
            key = state['source_file']
            clean = self.project(key, state)
            if clean:
                lifecycle_changed |= self.entries.get(key, {}).get('task_time') != clean['task_time']
                self.entries[key] = clean
        try:
            self.store.save(json.dumps({'version':1, 'entries':self.entries, 'skills':list(skills.values())}, separators=(',', ':')).encode())
        except (OSError, sqlite3.Error):
            self.write_health = 'unavailable'
        recent = sorted(skills.values(), key=lambda entry:entry['timestamp'], reverse=True)[:self.SKILL_LIMIT]
        lifecycle_changed |= recent != self.skills
        self.skills = recent
        if not lifecycle_changed and time.monotonic() < self.next_save:
            return
        self.next_save = time.monotonic()+30
        self.entries = dict(sorted(self.entries.items(), key=lambda item:item[1]['task_time'], reverse=True)[:self.LIMIT])
        raw = json.dumps({'version':1, 'entries':self.entries, 'skills':self.skills}, separators=(',', ':')).encode()
        while len(raw) > self.BYTE_LIMIT and self.entries:
            self.entries.pop(next(reversed(self.entries)))
            raw = json.dumps({'version':1, 'entries':self.entries, 'skills':self.skills}, separators=(',', ':')).encode()
        if raw == self.saved:
            return
        try:
            self.store.save(raw)
            self.saved = raw
            self.write_health = "ok"
        except (OSError, sqlite3.Error):
            self.write_health = "unavailable"
