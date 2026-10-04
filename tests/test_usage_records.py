import json
from pathlib import Path
import tempfile
import unittest

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.codex_metadata import read_metadata
from local_activity_monitor.usage_records import allowance
from local_activity_monitor.error_records import diagnostic_details, tool_error


class UsageTests(unittest.TestCase):
    def test_allowance_projects_dynamic_windows_and_zero_without_private_fields(self):
        result = allowance({'primary': {'used_percent': 15.0, 'window_minutes': 10080, 'resets_at': 1791608814}, 'secondary': {'used_percent': 0, 'window_minutes': 300}, 'credits': {'balance': '181.0192080000', 'unlimited': False, 'has_credits': True, 'secret': 'PRIVATE'}, 'plan_type': 'pro', 'account_id': 'PRIVATE'}, '2026-10-04T00:00:00Z')
        self.assertEqual(result['limits'][0]['remaining_percent'], 85)
        self.assertEqual(result['limits'][0]['window_minutes'], 10080)
        self.assertEqual(result['limits'][1]['used_percent'], 0)
        self.assertEqual(result['credits']['balance'], 181.019208)
        self.assertNotIn('PRIVATE', json.dumps(result))
        self.assertIsNone(allowance({'primary': {'used_percent': True, 'window_minutes': -1, 'resets_at': float('nan')}, 'credits': {'balance': 'NaN'}}, 'now'))
        self.assertNotIn('balance', allowance({'credits': {'has_credits': False, 'balance': None}}, 'now')['credits'])

    def test_collector_uses_latest_allowance_and_disable_hides_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'sessions';root.mkdir()
            path=root/'rollout-00000000-0000-4000-8000-000000000001.jsonl'
            collector=CodexCollector(root)
            state=collector.new_file(path)
            for when, used in [('2026-10-04T01:00:00Z', 25), ('2026-10-04T00:00:00Z', 10)]:
                collector.consume(state, {'type':'event_msg','timestamp':when,'payload':{'type':'token_count','rate_limits':{'primary':{'used_percent':used,'window_minutes':10080}}}})
            collector.files[path]=state
            self.assertEqual(collector.snapshot()['usage']['limits'][0]['used_percent'],25)
            collector.features['usage']=False
            self.assertIsNone(collector.snapshot()['usage'])

    def test_dots_only_selects_output_metadata_and_counts_activity_items(self):
        with tempfile.TemporaryDirectory() as directory:
            home=Path(directory);identity='00000000-0000-4000-8000-000000000001'
            output={'threadId':identity,'accountId':'PRIVATE','turnId':'PRIVATE-TURN','artifact':{'type':'image','path':'PRIVATE-PATH','producedAtMs':1791086400000}}
            (home/'.codex-global-state.json').write_text(json.dumps({'electron-persisted-atom-state':{'orbit-outputs-v1':[output, output], 'orbit-activity-snapshots-v1':[{'accountId':'PRIVATE','data':[]}]} }),encoding='utf-8')
            dots={};read_metadata(home, [], dots)
            self.assertEqual(len(dots['events']),1)
            self.assertEqual(dots['activity_items'],0)
            self.assertEqual(dots['events'][0]['artifact_type'],'image')
            self.assertNotIn('PRIVATE', json.dumps(dots))

    def test_error_trace_projection_has_cause_and_identifiers_without_message(self):
        message='Request timed out HTTP status=504 request_id=req_demo traceId=trace_demo secret=PRIVATE prompt=PRIVATE'
        fields=diagnostic_details(message)
        self.assertEqual(fields['cause'],'timeout')
        self.assertEqual(fields['http_status'],504)
        self.assertEqual(fields['request_id'],'req_demo')
        result=tool_error({'error':{'code':'upstream_failed','message':message}})
        self.assertEqual(result['trace_id'],'trace_demo')
        self.assertNotIn('PRIVATE',json.dumps(result))
        hidden=diagnostic_details('Failure errorMessage="PRIVATE request_id=req_private timeout" traceId=trace_actual')
        self.assertNotIn('request_id',hidden)
        self.assertNotIn('cause',hidden)
        self.assertEqual(hidden['trace_id'],'trace_actual')
