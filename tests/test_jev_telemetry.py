import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch
from datetime import datetime, timedelta, timezone

from local_activity_monitor.collectors import JevCollector
from local_activity_monitor.server import Dashboard


class JevTelemetryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
        self.database = self.home/'events.sqlite3'
        directory = self.home/'monitoring'
        directory.mkdir()
        (directory/'jev-monitor.json').write_text(json.dumps({'version':1,'enabled':True,'database':str(self.database)}))
        self.time = (datetime.now(timezone.utc)-timedelta(hours=2)).isoformat().replace('+00:00','Z')
        with closing(sqlite3.connect(self.database)) as db:
            db.execute('CREATE TABLE jev_events (timestamp TEXT, status TEXT, http_attempts INTEGER, input_tokens INTEGER, output_tokens INTEGER, request_bytes INTEGER, response_bytes INTEGER, response_unknown_attempts INTEGER, latency_ms INTEGER, metadata TEXT)')
            event = {'timestamp':self.time,'status':'future_status','operation':'new_operation','source':'new_source',
                     'input_tokens':23,'output_tokens':8,'latency_ms':9,'request_bytes':250,'response_bytes':80,
                     'requested_model':'fixture-model','prompt':'PRIVATE','api_key':'SECRET',
                     'metrics':{'credits_remaining':3.5,'saved_tokens':75,'secret_tokens':999},
                     'attempts':[{'status':'future_status','http_status':200,'latency_ms':1,'body':'PRIVATE'}]*5}
            db.execute('INSERT INTO jev_events VALUES (?,?,?,?,?,?,?,?,?,?)',(self.time,'future_status',5,23,8,250,80,0,9,json.dumps(event)))
            db.commit()

    def tearDown(self):
        self.temp.cleanup()

    def test_check_time_survives_telemetry_projection(self):
        dashboard = Dashboard(self.home)
        dashboard.refresh()
        snapshot = dashboard.snapshot('all')
        checked = snapshot['jev']['reader']['checked_at']
        self.assertEqual(snapshot['mcp']['telemetry']['jev']['reader']['checked_at'], checked)
        self.assertEqual(snapshot['sources']['telemetry:jev']['checked_at'], checked)
        self.assertEqual(dashboard.snapshot('all')['sources']['telemetry:jev']['checked_at'], checked)
        self.assertEqual(next(source for source in dashboard.logs()['sources'] if source['source']=='jev_telemetry')['checked_at'], checked)

    def test_unknown_metadata_is_projected_and_private_payloads_are_omitted(self):
        result = JevCollector(self.home).snapshot('all')
        self.assertEqual(result['health'],'ok')
        self.assertTrue(datetime.fromisoformat(result['reader']['checked_at'].replace('Z', '+00:00')).tzinfo)
        self.assertEqual(result['summary']['statuses'],{'future_status':1})
        self.assertEqual(result['summary']['retries'],4)
        event = result['recent'][0]
        self.assertEqual(event['operation'],'new_operation')
        self.assertEqual(event['source'],'new_source')
        self.assertEqual(event['request_bytes'],250)
        self.assertEqual(len(event['attempts']),5)
        self.assertEqual(event['metrics_credits_remaining'],3.5)
        self.assertEqual(event['metrics_saved_tokens'],75)
        self.assertNotIn('secret_tokens',json.dumps(result))
        self.assertNotIn('PRIVATE',json.dumps(result))
        self.assertNotIn('SECRET',json.dumps(result))

    def test_source_health_distinguishes_disabled_missing_invalid_and_empty(self):
        config = self.home/'monitoring/jev-monitor.json'
        config.write_text(json.dumps({'version':1,'enabled':False,'database':str(self.database)}))
        with patch('local_activity_monitor.collectors.sqlite3.connect', side_effect=AssertionError('Disabled reader must not open SQLite')):
            self.assertEqual(JevCollector(self.home).snapshot('all')['health'], 'disabled')
        missing = self.home/'unrecorded.sqlite3'
        config.write_text(json.dumps({'version':1,'enabled':True,'database':str(missing)}))
        result = JevCollector(self.home).snapshot('all')
        self.assertEqual(result['health'], 'not_recorded')
        self.assertEqual(result['reader']['locations'], [str(missing)])
        self.assertEqual(result['reader']['config_location'], str(config))
        self.assertFalse(missing.exists())
        config.write_text('invalid JSON')
        self.assertEqual(JevCollector(self.home).snapshot('all')['health'], 'invalid_config')
        config.write_text(json.dumps({'version':1,'enabled':True,'database':str(self.database)}))
        with closing(sqlite3.connect(self.database)) as db, db:
            db.execute('DELETE FROM jev_events')
        result = JevCollector(self.home).snapshot('all')
        self.assertEqual(result['health'], 'ok')
        self.assertEqual(result['summary']['calls'], 0)

    def test_source_failures_expose_types_without_private_error_text(self):
        for error, health in [(PermissionError('PRIVATE_PATH'), 'unavailable'), (sqlite3.OperationalError('database is locked'), 'unavailable'), (sqlite3.OperationalError('no such table: jev_events'), 'unsupported'), (sqlite3.OperationalError('no such column: http_attempts'), 'unsupported')]:
            with self.subTest(health=health, error=type(error).__name__), patch('local_activity_monitor.collectors.sqlite3.connect', side_effect=error):
                result = JevCollector(self.home).snapshot('all')
            self.assertEqual(result['health'], health)
            self.assertEqual(result['reader']['error_type'], type(error).__name__)
            self.assertNotIn('PRIVATE_PATH', json.dumps(result))

    def test_malformed_metadata_is_isolated_from_valid_rows(self):
        with closing(sqlite3.connect(self.database)) as db:
            for raw in (None,'null','[]','{bad'):
                db.execute('INSERT INTO jev_events VALUES (?,?,?,?,?,?,?,?,?,?)',(self.time,'ok',0,None,None,0,0,0,None,raw))
            db.commit()
        result = JevCollector(self.home).snapshot('all')
        self.assertEqual(result['health'],'ok')
        self.assertEqual(result['summary']['calls'],5)
        self.assertEqual(len(result['recent']),1)

    def test_empty_window_keeps_unknown_latency_and_does_not_mix_windows(self):
        dashboard = Dashboard(self.home)
        dashboard.refresh()
        hour = dashboard.snapshot('1h')['mcp']['telemetry']['jev']
        day = dashboard.snapshot('24h')['mcp']['telemetry']['jev']
        self.assertEqual(hour['summary']['calls'],0)
        self.assertIsNone(hour['summary']['average_latency_ms'])
        self.assertEqual(day['summary']['calls'],1)
        day['recent'][0]['attempts'].clear()
        self.assertEqual(len(dashboard.snapshot('24h')['mcp']['telemetry']['jev']['recent'][0]['attempts']),5)
        self.assertEqual(dashboard.snapshot('1h')['mcp']['telemetry']['jev']['recent'],[])
