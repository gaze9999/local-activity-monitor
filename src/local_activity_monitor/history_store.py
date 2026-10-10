"""Durable projected history with bounded readers and independent writers."""
from contextlib import closing
import json
import hashlib
import base64
import os
from pathlib import Path
import sqlite3
import stat
import time


APPLICATION_ID = 0x4C414D31
SCHEMA_VERSION = 3
MIGRATIONS = {1: (
    "CREATE TABLE history_state (namespace TEXT PRIMARY KEY, metadata TEXT NOT NULL, updated_at INTEGER NOT NULL)",
    "CREATE TABLE history_items (namespace TEXT NOT NULL, section TEXT NOT NULL, item_key TEXT NOT NULL, ordinal INTEGER NOT NULL, event_time TEXT, payload TEXT NOT NULL, PRIMARY KEY(namespace, section, item_key))",
    "CREATE INDEX history_time ON history_items(namespace, section, event_time)",
), 2: ("CREATE INDEX history_recent ON history_items(namespace, section, event_time DESC, item_key DESC)", "CREATE INDEX history_cleanup ON history_items(event_time)" ),
    3: ("CREATE TABLE session_cursors (source_key TEXT PRIMARY KEY, offset INTEGER NOT NULL, signature TEXT NOT NULL, payload TEXT NOT NULL)",
        "CREATE TABLE session_calls (source_key TEXT NOT NULL, call_id TEXT NOT NULL, event_time TEXT, payload TEXT NOT NULL, PRIMARY KEY(source_key,call_id))",
        "CREATE INDEX session_calls_recent ON session_calls(source_key,event_time DESC,call_id)",)}
SECTIONS = {"activity": {"sql": list, "web": list, "mcp": list},
            "errors": {"events": list}, "threads": {"entries": dict, "skills": list}}


def unsafe(path):
    if path.is_symlink():
        return True
    try:
        return bool(getattr(path.lstat(), "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))
    except FileNotFoundError:
        return False


class HistoryStore:
    def __init__(self, legacy, namespace):
        self.legacy = Path(legacy)
        self.path = self.legacy.with_name("lam-history.sqlite3")
        self.namespace = namespace
        self.sections = SECTIONS[namespace]
        self.imported = False

    def check_paths(self):
        for path in (self.path.parent, self.path, *(self.path.with_name(self.path.name + suffix) for suffix in ("-journal", "-wal", "-shm"))):
            if unsafe(path):
                raise OSError("Unsafe history storage")

    def connect(self, write=False):
        self.check_paths()
        if write:
            self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            self.check_paths()
        db = sqlite3.connect(self.path.resolve().as_uri() + ("?mode=rwc" if write else "?mode=ro"), uri=True, timeout=.2)
        try:
            application = db.execute("PRAGMA application_id").fetchone()[0]
            version = db.execute("PRAGMA user_version").fetchone()[0]
            if write and application == 0 and version == 0:
                if db.execute("SELECT 1 FROM sqlite_master LIMIT 1").fetchone():
                    raise sqlite3.DatabaseError("Unrecognized history database")
                self.migrate(db, 0)
                application, version = APPLICATION_ID, SCHEMA_VERSION
                if os.name != "nt":
                    self.path.chmod(0o600)
            if application == APPLICATION_ID and 0 < version < SCHEMA_VERSION:
                if not write:
                    db.close()
                    with closing(self.connect(write=True)):
                        pass
                    return self.connect()
                self.migrate(db, version)
                version = SCHEMA_VERSION
            if application != APPLICATION_ID or version != SCHEMA_VERSION:
                raise sqlite3.DatabaseError("Unsupported history database version")
            if write:
                db.execute("PRAGMA journal_mode=WAL")
                db.execute("PRAGMA synchronous=NORMAL")
            else:
                db.execute("PRAGMA query_only=ON")
            return db
        except BaseException:
            db.close()
            raise

    def migrate(self, db, version):
        if any(target not in MIGRATIONS for target in range(version + 1, SCHEMA_VERSION + 1)):
            raise sqlite3.DatabaseError("Missing history migration")
        if version:
            # Preserve an existing complete backup after a failed upgrade/retry.
            backup = self.path.with_name(self.path.name + ".schema-v" + str(version) + ".bak")
            if unsafe(backup):
                raise OSError("Unsafe history backup")
            if not backup.exists():
                with backup.open("xb"):
                    pass
                if os.name != "nt":
                    backup.chmod(0o600)
                with closing(sqlite3.connect(backup)) as target:
                    db.backup(target)
            with closing(sqlite3.connect(backup.resolve().as_uri() + "?mode=ro", uri=True)) as saved:
                if saved.execute("PRAGMA application_id").fetchone()[0] != APPLICATION_ID or saved.execute("PRAGMA user_version").fetchone()[0] != version or saved.execute("PRAGMA quick_check").fetchone()[0] != "ok":
                    raise sqlite3.DatabaseError("Invalid history migration backup")
        with db:
            db.execute("BEGIN IMMEDIATE")
            for target in range(version + 1, SCHEMA_VERSION + 1):
                for statement in MIGRATIONS[target]:
                    db.execute(statement)
                if target == 2:
                    rows = db.execute("SELECT namespace, section, item_key, ordinal, event_time, payload FROM history_items").fetchall()
                    db.execute("DELETE FROM history_items")
                    for namespace, section, key, ordinal, stamp, payload in rows:
                        stable = key if SECTIONS[namespace][section] is dict else self.item_key(namespace, section, json.loads(payload))
                        if isinstance(stamp, str):
                            from datetime import datetime, timezone
                            parsed = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
                            if not parsed.tzinfo:
                                raise ValueError("Invalid history timestamp")
                            stamp = parsed.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
                        db.execute("INSERT OR REPLACE INTO history_items VALUES (?, ?, ?, ?, ?, ?)", (namespace, section, stable, ordinal, stamp, payload))
            db.execute("PRAGMA application_id=" + str(APPLICATION_ID))
            db.execute("PRAGMA user_version=" + str(SCHEMA_VERSION))

    def load(self, byte_limit):
        self.check_paths()
        if self.path.exists():
            with closing(self.connect()) as db:
                row = db.execute("SELECT metadata FROM history_state WHERE namespace=?", (self.namespace,)).fetchone()
                if row:
                    if not isinstance(row[0], str) or len(row[0]) > 65536:
                        raise ValueError("Invalid history state")
                    value = json.loads(row[0])
                    if not isinstance(value, dict):
                        raise ValueError("Invalid history state")
                    value.update({name: kind() for name, kind in self.sections.items()})
                    total = len(row[0])
                    # Working-set budgets are not persistence limits. Read newest items
                    # independently per section so one busy source cannot starve another.
                    for section, kind in self.sections.items():
                        budget = max(0, (byte_limit-len(row[0])-1024)//len(self.sections))
                        used = 0
                        rows = db.execute("SELECT item_key, payload FROM history_items WHERE namespace=? AND section=? ORDER BY event_time DESC, item_key LIMIT 1000", (self.namespace, section))
                        for key, payload in rows:
                            if not isinstance(payload, str):
                                raise ValueError("Invalid history items")
                            used += len(payload)+len(key)+8
                            if used > budget:
                                break
                            item = json.loads(payload)
                            if kind is dict:
                                value[section][key] = item
                            else:
                                value[section].append(item)
                    return json.dumps(value, ensure_ascii=True, separators=(",", ":")).encode()
        if self.legacy == self.path:
            return None
        if unsafe(self.legacy):
            raise OSError("Unsafe legacy history")
        if self.legacy.exists():
            with self.legacy.open("rb") as stream:
                raw = stream.read(byte_limit + 1)
            self.imported = True
            return raw
        return None

    def save(self, raw):
        value = json.loads(raw)
        metadata = {key: item for key, item in value.items() if key not in self.sections}
        rows = []
        for section, kind in self.sections.items():
            items = value.get(section, kind())
            if not isinstance(items, kind):
                raise ValueError("Invalid history section")
            entries = items.items() if kind is dict else enumerate(items)
            for ordinal, (key, item) in enumerate(entries):
                stable = str(key) if kind is dict else self.item_key(self.namespace, section, item)
                stamp = item.get("timestamp", item.get("task_time"))
                if isinstance(stamp, str):
                    from datetime import datetime, timezone
                    stamp = datetime.fromisoformat(stamp.replace("Z", "+00:00")).astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
                rows.append((self.namespace, section, stable, ordinal, stamp,
                             json.dumps(item, ensure_ascii=True, separators=(",", ":"))))
        with closing(self.connect(write=True)) as db, db:
            from datetime import datetime, timedelta, timezone
            settings = db.execute("SELECT metadata FROM history_state WHERE namespace='activity'").fetchone()
            days = metadata.get('retention_days', 90) if self.namespace == 'activity' else json.loads(settings[0]).get('retention_days', 90) if settings else 90
            boundary = (datetime.now(timezone.utc)-timedelta(days=days)).isoformat(timespec='milliseconds').replace('+00:00', 'Z') if days else None
            rows = [row for row in rows if row[1] == 'entries' or boundary is None or row[4] is None or row[4] >= boundary]
            if self.namespace == 'activity':
                from .activity_history import merge_event
                previous = {}
                for section in self.sections:
                    keys = list(dict.fromkeys(row[2] for row in rows if row[1] == section))
                    for start in range(0, len(keys), 200):
                        batch = keys[start:start+200]
                        placeholders = ','.join('?' for _ in batch)
                        for key, payload in db.execute('SELECT item_key, payload FROM history_items WHERE namespace=? AND section=? AND item_key IN ('+placeholders+')', (self.namespace, section, *batch)):
                            previous[section, key] = payload
                rows = [(*row[:5], json.dumps(merge_event(json.loads(previous[row[1], row[2]]), json.loads(row[5])), ensure_ascii=True, separators=(',', ':'))) if (row[1], row[2]) in previous else row for row in rows]
            db.execute("INSERT INTO history_state (namespace, metadata, updated_at) VALUES (?, ?, ?) ON CONFLICT(namespace) DO UPDATE SET metadata=excluded.metadata, updated_at=excluded.updated_at WHERE history_state.metadata != excluded.metadata",
                       (self.namespace, json.dumps(metadata, separators=(",", ":")), time.time_ns()))
            db.executemany("INSERT INTO history_items VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(namespace, section, item_key) DO UPDATE SET event_time=excluded.event_time, payload=excluded.payload WHERE history_items.payload != excluded.payload", rows)

    @staticmethod
    def item_key(namespace, section, item):
        if namespace == "activity":
            from .activity_history import sql_identity, web_identity, mcp_identity
            identity = {"sql": sql_identity, "web": web_identity, "mcp": mcp_identity}[section](item)
        elif namespace == "errors":
            from .error_history import error_identity
            identity = error_identity(item)
        else:
            identity = tuple(item.get(key) for key in ("thread_id", "call_id", "skill"))
        return hashlib.sha256(json.dumps(identity, separators=(",", ":")).encode()).hexdigest()

    def page(self, section, *, limit=100, offset=0, since=None, cursor=None, excluded_sources=()):
        if section not in self.sections or type(limit) is not int or not 1 <= limit <= 200 or type(offset) is not int or not 0 <= offset <= 1000000:
            raise ValueError("Invalid history query")
        where, args = "namespace=? AND section=?", [self.namespace, section]
        if not isinstance(excluded_sources, (list, tuple)) or len(excluded_sources)>64 or any(not isinstance(value,str) or not 1<=len(value)<=80 for value in excluded_sources):
            raise ValueError("Invalid excluded sources")
        if excluded_sources:
            where += " AND (json_extract(payload, '$.server') IS NULL OR json_extract(payload, '$.server') NOT IN ("+','.join('?' for _ in excluded_sources)+"))"
            args.extend(excluded_sources)
        after = None
        if cursor is not None:
            try:
                if not isinstance(cursor, str) or not 1 <= len(cursor) <= 400 or offset:
                    raise ValueError("Invalid history cursor")
                after = json.loads(base64.b64decode(cursor.encode('ascii'), altchars=b'-_', validate=True))
                if not isinstance(after, list) or len(after) != 2 or not all(isinstance(value, str) for value in after) or not 0 < len(after[1]) <= 256 or len(after[0]) > 40:
                    raise ValueError("Invalid history cursor")
                if after[0]:
                    from datetime import datetime
                    if not datetime.fromisoformat(after[0].replace('Z', '+00:00')).tzinfo:
                        raise ValueError("Invalid history cursor")
            except (ValueError, TypeError, UnicodeError) as error:
                raise ValueError("Invalid history cursor") from error
        if since is not None:
            from datetime import datetime, timezone
            stamp = datetime.fromisoformat(since.replace("Z", "+00:00"))
            if not stamp.tzinfo:
                raise ValueError("Invalid history date")
            where += " AND event_time>=?"
            args.append(stamp.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z"))
        if not self.path.exists():
            return {"items": [], "total": 0, "limit": limit, "offset": offset, "next_cursor": None}
        with closing(self.connect()) as db, db:
            db.execute("BEGIN")
            total = db.execute("SELECT count(*) FROM history_items WHERE "+where, args).fetchone()[0]
            columns = "SELECT payload, event_time, item_key FROM history_items WHERE "
            order = " ORDER BY event_time DESC, item_key DESC LIMIT ?"
            if after is None:
                rows = db.execute(columns+where+order+" OFFSET ?", (*args, limit+1, offset)).fetchall()
            elif after[0]:
                rows = db.execute(columns+where+" AND (event_time, item_key) < (?, ?)"+order, (*args, *after, limit+1)).fetchall()
                if len(rows) <= limit and since is None:
                    rows += db.execute(columns+where+" AND event_time IS NULL"+order, (*args, limit+1-len(rows))).fetchall()
            else:
                rows = db.execute(columns+where+" AND event_time IS NULL AND item_key<?"+order, (*args, after[1], limit+1)).fetchall()
        more = len(rows) > limit
        rows = rows[:limit]
        next_cursor = base64.urlsafe_b64encode(json.dumps([rows[-1][1] or '', rows[-1][2]], separators=(',', ':')).encode()).decode() if more else None
        return {"items": [json.loads(row[0]) for row in rows], "total": total, "limit": limit, "offset": offset, "next_cursor": next_cursor}

    def aggregate(self, section, since=None, excluded_sources=()):
        """Aggregate retained metadata in SQLite without loading event payloads."""
        return self.aggregate_many(section, [since], excluded_sources)[0]

    def aggregate_many(self, section, boundaries, excluded_sources=()):
        """Read several time ranges in one bounded, consistent transaction."""
        from datetime import datetime, timezone
        if section not in self.sections or not isinstance(boundaries, (list, tuple)) or not 1 <= len(boundaries) <= 16:
            raise ValueError("Invalid history query")
        if not isinstance(excluded_sources, (list, tuple)) or len(excluded_sources)>64 or any(not isinstance(value, str) or not 1<=len(value)<=80 for value in excluded_sources):
            raise ValueError("Invalid excluded sources")
        where, args = "namespace=? AND section=?", [self.namespace, section]
        if excluded_sources:
            where += " AND (json_extract(payload, '$.server') IS NULL OR json_extract(payload, '$.server') NOT IN ("+','.join('?' for _ in excluded_sources)+"))"
            args.extend(excluded_sources)
        queries = []
        for since in boundaries:
            if since is None:
                queries.append((where, args))
                continue
            try:
                stamp = datetime.fromisoformat(since.replace('Z', '+00:00'))
                if not stamp.tzinfo:
                    raise ValueError("Invalid history date")
                boundary = stamp.astimezone(timezone.utc).isoformat(timespec='milliseconds').replace('+00:00', 'Z')
            except (ValueError, TypeError, AttributeError) as error:
                raise ValueError("Invalid history date") from error
            queries.append((where+" AND event_time>=?", [*args, boundary]))
        if not self.path.exists():
            return [self._aggregate(None, *query) for query in queries]
        with closing(self.connect()) as db, db:
            db.execute('BEGIN')
            return [self._aggregate(db, *query) for query in queries]

    @staticmethod
    def _aggregate(db, where, args):
        from datetime import datetime, timezone
        import math
        result = {'total':0, 'dated_total':0, 'undated_total':0, 'operations':{}, 'statements':{}, 'servers':{}, 'series':[], 'bucket_seconds':60, 'classification_truncated':False}
        if db is None:
            return result
        epoch = "CAST(strftime('%s',event_time) AS INTEGER)"
        nested = "CASE WHEN json_extract(payload,'$.nested')=1 THEN 1 ELSE 0 END"
        total, dated, earliest, latest = db.execute('SELECT count(*), count('+epoch+'), min('+epoch+'), max('+epoch+') FROM history_items WHERE '+where, args).fetchone()
        result.update(total=total, dated_total=dated, undated_total=total-dated)
        if not total:
            return result
        for field in ('operation', 'statement'):
            column = "json_extract(payload,'$."+field+"')"
            rows = db.execute('SELECT '+column+', count(*) FROM history_items WHERE '+where+' AND '+column+' IS NOT NULL GROUP BY '+column+' ORDER BY count(*) DESC, '+column+' LIMIT 201', args).fetchall()
            result[field+'s'] = {name:count for name,count in rows[:200]}
            result['classification_truncated'] |= len(rows)>200
        column = "json_extract(payload,'$.server')"
        rows = db.execute('SELECT '+column+', count(*), sum('+nested+') FROM history_items WHERE '+where+' AND '+column+' IS NOT NULL GROUP BY '+column+' ORDER BY count(*) DESC, '+column+' LIMIT 201', args).fetchall()
        result['servers'] = {name:{'total':count, 'direct':count-inferred, 'nested':inferred} for name,count,inferred in rows[:200]}
        result['classification_truncated'] |= len(rows)>200
        if dated:
            minutes = latest//60-earliest//60+1
            bucket = max(1, math.ceil(minutes/1439))*60
            result['bucket_seconds'] = bucket
            rows = db.execute('SELECT ('+epoch+'/?) * ?, count(*), sum('+nested+') FROM history_items WHERE '+where+' AND '+epoch+' IS NOT NULL GROUP BY 1 ORDER BY 1 LIMIT 1440', (bucket,bucket,*args)).fetchall()
            result['series'] = [{'time':datetime.fromtimestamp(stamp,timezone.utc).isoformat(timespec='seconds').replace('+00:00','Z'), 'calls':count, 'direct':count-inferred, 'nested':inferred, 'operations':{}, 'servers':{}} for stamp,count,inferred in rows]
            slots = {stamp:item for (stamp,_,_),item in zip(rows,result['series'])}
            result['classification_series_truncated'] = False
            for field in ('operation', 'server'):
                column = "json_extract(payload,'$."+field+"')"
                grouped = db.execute('SELECT ('+epoch+'/?) * ?, '+column+', count(*), sum('+nested+') FROM history_items WHERE '+where+' AND '+epoch+' IS NOT NULL AND '+column+' IS NOT NULL GROUP BY 1, 2 ORDER BY 1, 2 LIMIT 14401', (bucket,bucket,*args)).fetchall()
                result['classification_series_truncated'] |= len(grouped)>14400
                for stamp,name,count,inferred in grouped[:14400]:
                    slots[stamp][field+'s'][name] = {'total':count,'direct':count-inferred,'nested':inferred} if field=='server' else count
        return result

    def prune(self, days, batch=500):
        """Opt-in, bounded cleanup shared by all history namespaces."""
        if type(days) is not int or not 0 <= days <= 3650:
            raise ValueError("Invalid history retention")
        if not days or not self.path.exists():
            return 0
        from datetime import datetime, timedelta, timezone
        boundary = (datetime.now(timezone.utc)-timedelta(days=days)).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        with closing(self.connect(write=True)) as db, db:
            cursor = db.execute("DELETE FROM history_items WHERE rowid IN (SELECT rowid FROM history_items WHERE event_time<? AND section!='entries' ORDER BY event_time LIMIT ?)", (boundary, batch))
            return cursor.rowcount

    def read_document(self, byte_limit):
        return json.loads(self.load(byte_limit))
