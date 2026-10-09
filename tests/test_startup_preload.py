import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.server import Dashboard
from local_activity_monitor.codex_metadata import read_metadata, CATALOG_LIMIT


class StartupPreloadTests(unittest.TestCase):
    def test_catalog_ready_without_reading_sessions_or_claiming_complete(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            (home/'sessions').mkdir()
            identity = '00000000-0000-4000-8000-000000000001'
            session = home/'sessions'/f'rollout-{identity}.jsonl'
            session.write_text('PRIVATE_SESSION_BODY', encoding='utf-8')
            with closing(sqlite3.connect(home/'state_5.sqlite')) as db, db:
                db.execute('CREATE TABLE threads(id,title,updated_at,prompt,token)')
                db.execute('INSERT INTO threads VALUES(?,?,?,?,?)', (identity,'Catalog title',1780000000,'PRIVATE_PROMPT','PRIVATE_CREDENTIAL'))
            dashboard = Dashboard(home, codex=True)
            with patch.object(dashboard.codex, 'refresh', side_effect=AssertionError('session refresh')), patch.object(dashboard.monitor, 'load_device', side_effect=AssertionError('device scan')):
                dashboard.preload()
                snapshot = dashboard.snapshot('all')
            self.assertIsNone(snapshot['updated_at'])
            self.assertEqual(snapshot['initial_sections'], ['catalog', 'runtime'])
            self.assertEqual(snapshot['codex']['bytes_read'], 0)
            self.assertEqual(snapshot['codex']['threads'][0]['thread_name'], 'Catalog title')
            serialized = json.dumps(snapshot)
            for private in ('PRIVATE_PROMPT','PRIVATE_CREDENTIAL','PRIVATE_SESSION_BODY'):
                self.assertNotIn(private, serialized)
            self.assertEqual(dashboard.update_version, 0)

    def test_recent_catalog_is_bounded_and_normal_reads_remain_selected(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            with closing(sqlite3.connect(home/'state_5.sqlite')) as db, db:
                db.execute('CREATE TABLE threads(id,title,updated_at)')
                db.executemany('INSERT INTO threads VALUES(?,?,?)', ((f'00000000-0000-4000-8000-{i:012d}',f'Thread {i}',i) for i in range(CATALOG_LIMIT+15)))
            self.assertEqual(read_metadata(home, []), {})
            entries = read_metadata(home, [], select_recent=True)
            self.assertEqual(len(entries), CATALOG_LIMIT)
            self.assertIn('00000000-0000-4000-8000-000000002014', entries)
            self.assertNotIn('00000000-0000-4000-8000-000000000000', entries)
            with closing(sqlite3.connect(home/'state_4.sqlite')) as db, db:
                db.execute('CREATE TABLE threads(id,title,updated_at)')
                db.execute('INSERT INTO threads VALUES(?,?,?)', ('00000000-0000-4000-8000-000000099999','Older database',1))
            self.assertEqual(len(read_metadata(home, [], select_recent=True)), CATALOG_LIMIT)

    def test_disabled_codex_does_not_read_catalog(self):
        with tempfile.TemporaryDirectory() as folder:
            dashboard = Dashboard(Path(folder))
            dashboard.preload()
            self.assertEqual(dashboard.cache, {})
            self.assertIsNone(dashboard.snapshot('all')['updated_at'])

    def test_disabled_observation_does_not_preload_codex_metadata(self):
        with tempfile.TemporaryDirectory() as folder:
            dashboard = Dashboard(Path(folder), codex=True)
            dashboard.observations['codex'] = False
            with patch.object(dashboard.codex, 'snapshot_windows', side_effect=AssertionError('disabled read')):
                dashboard.preload()
            self.assertEqual(dashboard.cache, {})


if __name__ == '__main__':
    unittest.main()
