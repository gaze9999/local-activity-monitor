"""Persistence, opt-in cleanup and concurrent reader regression checks."""
from contextlib import closing
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from local_activity_monitor.activity_history import ActivityHistory
from local_activity_monitor.history_store import HistoryStore


class DurableHistoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)/'activity-history.json'
        self.history = ActivityHistory(self.path)
        self.stamp = datetime.now(timezone.utc).isoformat()

    def event(self, index, **changes):
        return {'timestamp': self.stamp, 'thread_id': '00000000-0000-4000-8000-000000000001', 'call_id': str(index), 'server': 'web', 'tool': 'web.run', **changes}

    def test_persistence_exceeds_working_set_without_duplicate_writes(self):
        self.history.update([], [self.event(index) for index in range(2500)])
        self.assertLessEqual(len(self.history.web), self.history.WEB_LIMIT)
        self.assertEqual(self.history.store.page('web')['total'], 2500)
        self.history.update([], [self.event(2500)])
        self.assertEqual(self.history.store.page('web', offset=2400)['total'], 2501)
        loaded = ActivityHistory(self.path)
        self.assertEqual(loaded.health, 'ok')
        self.assertLessEqual(len(loaded.store.load(loaded.BYTE_LIMIT)), loaded.BYTE_LIMIT)
        with closing(loaded.store.connect(write=True)) as db:
            with db:
                db.execute("CREATE TEMP TRIGGER unchanged_write BEFORE UPDATE ON history_items BEGIN SELECT RAISE(FAIL,'unexpected write'); END")
            # A held write transaction leaves the previous committed snapshot readable.
            db.execute('BEGIN IMMEDIATE')
            db.execute("UPDATE history_state SET updated_at=0")
            self.assertEqual(loaded.store.page('web')['total'], 2501)
            db.rollback()

    def test_database_aggregate_preserves_full_history_and_source_filters(self):
        events = [self.event(index, server='jev' if index%2 else 'codex_app', nested=index%3==0) for index in range(2501)]
        self.history.store.save(json.dumps({'version':3,'retention_days':0,'mcp':events+[self.event(3000, server='jev', timestamp=None)]}))
        result = self.history.store.aggregate('mcp')
        self.assertEqual(result['total'], 2502)
        self.assertEqual(result['dated_total'], 2501)
        self.assertEqual(result['undated_total'], 1)
        self.assertEqual(sum(row['calls'] for row in result['series']),2501)
        self.assertEqual(result['servers']['jev']['total'],1251)
        filtered = self.history.store.aggregate('mcp', excluded_sources=['jev'])
        self.assertEqual(filtered['total'],1251)
        self.assertEqual(set(filtered['servers']),{'codex_app'})
        self.assertEqual(self.history.store.aggregate('mcp',since=self.stamp)['total'],2501)
        self.assertEqual(self.history.store.aggregate('mcp',since=(datetime.now(timezone.utc)+timedelta(days=1)).isoformat())['total'],0)

    def test_database_aggregate_bounds_series_and_counts_sql_classifications(self):
        start = datetime.now(timezone.utc)-timedelta(days=10)
        events = [self.event(index, timestamp=(start+timedelta(minutes=index*5)).isoformat(), operation='query' if index%2 else 'write', statement='SELECT' if index%2 else 'INSERT') for index in range(2500)]
        self.history.store.save(json.dumps({'version':3,'retention_days':0,'sql':events}))
        result = self.history.store.aggregate('sql')
        self.assertLessEqual(len(result['series']),1440)
        self.assertGreater(result['bucket_seconds'],60)
        self.assertEqual(sum(row['calls'] for row in result['series']),2500)
        self.assertEqual(result['statements'],{'INSERT':1250,'SELECT':1250})
        self.assertEqual(result['operations'],{'query':1250,'write':1250})
        for params in ({'since':'invalid'},{'since':'2026-01-01'},{'excluded_sources':'jev'}):
            with self.assertRaises(ValueError):
                self.history.store.aggregate('sql',**params)

    def test_retention_zero_and_bounded_cleanup(self):
        self.history.retention_days = 0
        old = (datetime.now(timezone.utc)-timedelta(days=100)).isoformat()
        self.history.update([], [self.event(index, timestamp=old) for index in range(1200)])
        self.assertEqual(self.history.store.page('web')['total'], 1200)
        self.assertEqual(self.history.store.prune(0), 0)
        self.history.retention_days = 90
        self.history.update([], [])
        self.assertEqual(self.history.store.page('web')['total'], 700)
        self.history.update([], [self.event(0, timestamp=old)])
        self.assertEqual(self.history.store.page('web')['total'], 200)
        self.history.update([], [])
        self.assertEqual(self.history.store.page('web')['total'], 0)

    def test_read_connection_cannot_write_and_page_is_bounded(self):
        self.history.update([], [self.event(1)])
        with closing(self.history.store.connect()) as db:
            self.assertEqual(db.execute('PRAGMA journal_mode').fetchone()[0], 'wal')
            with self.assertRaises(sqlite3.OperationalError):
                db.execute('DELETE FROM history_items')
        for value in [0, 201, True, -1]:
            with self.assertRaises(ValueError):
                self.history.store.page('web', limit=value)

    def test_cursor_pages_ties_and_undated_records_without_duplicates(self):
        self.history.store.save(json.dumps({'version':3,'retention_days':0,'web':[self.event(index, timestamp=self.stamp if index<13 else None) for index in range(19)]}))
        seen, cursor = [], None
        while True:
            page = self.history.store.page('web', limit=4, cursor=cursor)
            self.assertEqual(page['total'],19)
            self.assertLessEqual(len(page['items']),4)
            seen.extend(row['call_id'] for row in page['items'])
            cursor = page['next_cursor']
            if cursor is None:
                break
        self.assertEqual(len(seen),19)
        self.assertEqual(len(set(seen)),19)
        dated = self.history.store.page('web', since=self.stamp, limit=200)
        self.assertEqual(len(dated['items']),13)
        self.assertIsNone(dated['next_cursor'])
        for cursor in ('?', 'a', '', 'W10=', 'bnVsbA=='):
            with self.assertRaises(ValueError):
                self.history.store.page('web',cursor=cursor)
        token = self.history.store.page('web',limit=1)['next_cursor']
        with self.assertRaises(ValueError):
            self.history.store.page('web',cursor=token,offset=1)

    def test_missing_database_still_validates_cursor_and_returns_terminal_page(self):
        with self.assertRaises(ValueError):
            self.history.store.page('web',cursor='?')
        self.assertIsNone(self.history.store.page('web')['next_cursor'])

    def test_disabled_sources_are_excluded_from_counts_and_pages(self):
        events = [self.event(index, server='hidden' if index%2 else 'visible') for index in range(13)]
        self.history.store.save(json.dumps({'version':3,'retention_days':0,'mcp':events}))
        page = self.history.store.page('mcp', limit=3, excluded_sources=['hidden'])
        self.assertEqual(page['total'], 7)
        self.assertTrue(all(event['server']=='visible' for event in page['items']))
        next_page = self.history.store.page('mcp', limit=3, cursor=page['next_cursor'], excluded_sources=['hidden'])
        self.assertTrue(all(event['server']=='visible' for event in next_page['items']))
        for invalid in ('hidden', [None], ['x'*81]):
            with self.assertRaises(ValueError):
                self.history.store.page('mcp', excluded_sources=invalid)

    def test_schema_one_upgrade_preserves_old_rows_and_backup(self):
        from local_activity_monitor import history_store
        legacy = Path(self.temp.name)/'old'
        legacy.mkdir()
        path = legacy/'lam-history.sqlite3'
        stamp = self.stamp
        with closing(sqlite3.connect(path)) as db, db:
            for statement in history_store.MIGRATIONS[1]:
                db.execute(statement)
            db.execute('PRAGMA application_id='+str(history_store.APPLICATION_ID))
            db.execute('PRAGMA user_version=1')
            db.execute('INSERT INTO history_state VALUES(?,?,?)', ('activity', json.dumps({'version':3,'retention_days':0}), 1))
            db.execute('INSERT INTO history_items VALUES(?,?,?,?,?,?)', ('activity','web','0',0,stamp,json.dumps(self.event(1))))
        store = HistoryStore(legacy/'activity-history.json','activity')
        self.assertEqual(store.page('web')['total'], 1)
        self.assertTrue(path.with_name(path.name+'.schema-v1.bak').exists())
        store.save(json.dumps({'version':3,'retention_days':0,'web':[self.event(1)]}))
        self.assertEqual(store.page('web')['total'], 1)


if __name__ == '__main__':
    unittest.main()
