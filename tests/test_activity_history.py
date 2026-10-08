from datetime import datetime,timedelta,timezone
import json
from pathlib import Path
import tempfile
import unittest

from local_activity_monitor.activity_history import ActivityHistory
from local_activity_monitor.server import Dashboard


class ActivityHistoryTests(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.home=Path(self.directory.name)
        self.path=self.home/'monitoring/activity-history.json'
        self.timestamp=datetime.now(timezone.utc).isoformat().replace('+00:00','Z')
        self.sql={'timestamp':self.timestamp,'statement':'SELECT','operation':'read','engine':'SQLite','recognition':'diagnostic_log','source':'codex_core','file':'logs_1.sqlite','record_id':1,'duration_ms':0,'sql':'PRIVATE_SQL'}
        self.web={'timestamp':self.timestamp,'server':'web','tool':'run','thread_id':'1'*36,'call_id':'call1','index':0,'metadata':{'references':['https://example.com/docs'],'request':'PRIVATE_REQUEST'},'result':{'references':['https://example.com/result'],'body':'PRIVATE_RESPONSE'}}

    def test_metadata_survives_empty_refreshes_and_restart_without_bodies(self):
        history=ActivityHistory(self.path)
        history.update([self.sql],[self.web])
        history.update([], [self.web | {'metadata': {'references': []}, 'result': {'references': []}}])
        history.update([],[])
        loaded=ActivityHistory(self.path)
        self.assertEqual(len(loaded.snapshot('sql')),1)
        self.assertEqual(len(loaded.snapshot('web')),1)
        self.assertEqual(loaded.web[0]['metadata']['references'], ['https://example.com/docs'])
        self.assertEqual(loaded.snapshot('sql')[0]['duration_ms'],0)
        self.assertNotIn('PRIVATE',json.dumps(history.store.read_document(history.BYTE_LIMIT)))

    def test_duplicates_expiry_and_limits_are_bounded(self):
        history=ActivityHistory(self.path)
        old=(datetime.now(timezone.utc)-timedelta(days=91)).isoformat()
        history.update([self.sql,self.sql,self.sql|{'timestamp':old}],[self.web,self.web])
        self.assertEqual(len(history.sql),1)
        self.assertEqual(len(history.web),1)
        history.update([self.sql|{'record_id':index} for index in range(700)],[self.web|{'call_id':str(index)} for index in range(1200)])
        self.assertEqual(len(history.sql),500)
        self.assertEqual(len(history.web),1000)
        self.assertLessEqual(len(history.store.load(history.BYTE_LIMIT)),history.BYTE_LIMIT)

    def test_dashboard_retains_sql_and_network_after_live_sources_disappear(self):
        app=Dashboard(self.home,codex=True)
        app.activity_history.update([self.sql],[self.web])
        app.refresh()
        value=app.snapshot('all')
        self.assertEqual(value['codex']['sqlite']['total'],1)
        self.assertEqual(len([event for event in value['mcp']['events'] if event['server']=='web']),1)
        app.refresh()
        self.assertEqual(app.snapshot('all')['codex']['sqlite']['total'],1)
        restarted=Dashboard(self.home,codex=True)
        restarted.refresh()
        self.assertEqual(restarted.snapshot('all')['codex']['sqlite']['total'],1)
        self.assertEqual(len(restarted.snapshot('all')['mcp']['events']),1)
        restarted.set_settings({'observations':{'sqlite':False,'web':False}})
        self.assertEqual(restarted.snapshot('all')['codex'].get('sqlite',{}).get('total',0),0)
        self.assertEqual(restarted.snapshot('all')['mcp']['events'],[])

    def test_unsupported_and_private_payload_fields_are_not_saved(self):
        history=ActivityHistory(self.path)
        history.update([self.sql|{'statement':'SELECT PRIVATE FROM users'}],[self.web|{'metadata':{'references':['http://private.internal','https://example.com/?token=PRIVATE']}}])
        self.assertEqual(history.sql,[])
        self.assertNotIn('PRIVATE',json.dumps(history.store.read_document(history.BYTE_LIMIT)))

    def test_mcp_metadata_survives_restart_and_incomplete_reloads(self):
        event = self.web | {'server': 'future', 'metadata': {'prompt': 'PRIVATE', 'resources': {'page_id': 'page1'}}, 'result': {'status': 'success', 'input_tokens': 0, 'cache_hit': False, 'password': 'PRIVATE'}}
        history = ActivityHistory(self.path)
        history.update([], [], [event])
        history.update([], [], [event | {'result': {}, 'completed_at': None}])
        self.assertNotIn('PRIVATE', json.dumps(history.store.read_document(history.BYTE_LIMIT)))
        loaded = ActivityHistory(self.path)
        self.assertEqual(loaded.mcp[0]['result']['input_tokens'], 0)
        self.assertFalse(loaded.mcp[0]['result']['cache_hit'])
        self.assertEqual(loaded.mcp[0]['metadata']['resources']['page_id'], 'page1')
        app = Dashboard(self.home, codex=True)
        app.refresh()
        self.assertEqual(app.snapshot('all')['mcp']['events'][0]['server'], 'future')
        app.set_settings({'observations': {'mcp': False}})
        self.assertEqual(app.snapshot('all')['mcp']['events'], [])

    def test_legacy_history_is_loaded(self):
        self.path.parent.mkdir()
        self.path.write_text(json.dumps({'version': 1, 'sql': [self.sql], 'web': [self.web]}))
        history = ActivityHistory(self.path)
        self.assertEqual(len(history.sql), 1)
        self.assertEqual(history.mcp, [])

    def test_version_two_migrates_and_retention_limits_are_reported(self):
        self.path.parent.mkdir()
        self.path.write_text(json.dumps({'version':2,'sql':[self.sql],'web':[self.web],'mcp':[]}))
        app=Dashboard(self.home,codex=True)
        app.refresh()
        saved=app.activity_history.store.read_document(ActivityHistory.BYTE_LIMIT)
        self.assertEqual(saved['version'],3)
        self.assertEqual(saved['retention_days'],90)
        self.assertEqual(json.loads(self.path.read_text())['version'],2)
        coverage=app.snapshot('all')['sources']['activity_history']
        self.assertEqual(coverage['limits']['retention_days'],90)

    def test_weekend_events_survive_with_default_ninety_days(self):
        stamp=(datetime.now(timezone.utc)-timedelta(days=6)).isoformat()
        history=ActivityHistory(self.path)
        history.update([self.sql|{'timestamp':stamp}],[self.web|{'timestamp':stamp}],[self.web|{'timestamp':stamp,'server':'future'}])
        app=Dashboard(self.home,codex=True)
        app.refresh()
        self.assertEqual(app.settings()['activity_retention_days'],90)
        self.assertEqual(app.snapshot('7d')['codex']['sqlite']['total'],1)
        self.assertEqual(len(app.snapshot('7d')['mcp']['events']),2)
        self.assertEqual(app.snapshot('24h')['codex']['sqlite']['total'],0)

    def test_custom_retention_persists_and_shortening_removes_expired_events(self):
        app=Dashboard(self.home,codex=True)
        app.set_settings({'activity_retention_days':14})
        old=(datetime.now(timezone.utc)-timedelta(days=10)).isoformat()
        app.activity_history.update([self.sql|{'timestamp':old}],[])
        restarted=Dashboard(self.home,codex=True)
        self.assertEqual(restarted.settings()['activity_retention_days'],14)
        self.assertEqual(len(restarted.activity_history.sql),1)
        restarted.set_settings({'activity_retention_days':7})
        self.assertEqual(restarted.activity_history.sql,[])
        self.assertEqual(ActivityHistory(self.path).retention_days,7)
        for invalid in (-1,3651,True,1.5,'7'):
            with self.assertRaises(ValueError):restarted.set_settings({'activity_retention_days':invalid})


if __name__=='__main__':unittest.main()
