from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.activity_windows import contains, cutoff
from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.server import Dashboard
from local_activity_monitor.mcp_records import summarize


class ActivityWindowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.root = self.home/'sessions'
        self.root.mkdir()
        self.reference = datetime.now(timezone.utc)

    def write(self):
        thread = '00000000-0000-0000-0000-000000000001'
        records = [{'type': 'session_meta', 'timestamp': self.reference.isoformat(), 'payload': {'id': thread}}]
        for index in range(130):
            # Retained old calls exceed table cap, recent calls remain visible after filtering.
            when = self.reference-timedelta(days=2) if index < 125 else self.reference-timedelta(minutes=5)
            records.append({'type': 'response_item', 'timestamp': when.isoformat(), 'payload': {
                'type': 'function_call', 'name': 'mcp__future__run', 'call_id': str(index), 'arguments': '{}'}})
            records.append({'type': 'response_item', 'timestamp': (when+timedelta(seconds=1)).isoformat(), 'payload': {
                'type': 'function_call_output', 'call_id': str(index), 'output': json.dumps({'recording_enabled': False, 'items_count': index})}})
        (self.root/f'rollout-{thread}.jsonl').write_text('\n'.join(map(json.dumps, records))+'\n', encoding='utf-8')

    def test_window_boundaries_and_unknown_timestamp(self):
        boundary = cutoff('1h', self.reference)
        self.assertTrue(contains({'timestamp': boundary.isoformat()}, boundary))
        self.assertFalse(contains({'timestamp': (boundary-timedelta(microseconds=1)).isoformat()}, boundary))
        for value in (None, '', 'invalid', '2026-10-05T00:00:00'):
            self.assertFalse(contains({'timestamp': value}, boundary))
            self.assertTrue(contains({'timestamp': value}, None))

    def test_scope_before_cap_and_metadata_only_once(self):
        self.write()
        collector = CodexCollector(self.root)
        collector.refresh()
        from local_activity_monitor.collectors import read_metadata
        before = collector.read_bytes
        with patch('local_activity_monitor.collectors.read_metadata', wraps=read_metadata) as metadata:
            reports = collector.snapshot_windows()
        metadata.assert_called_once()
        self.assertEqual(collector.read_bytes, before)
        self.assertEqual(reports['all']['observed_tool_calls'], 130)
        self.assertEqual(reports['1h']['observed_tool_calls'], 5)
        self.assertEqual(reports['24h']['tool_statistics'][0]['calls'], 5)
        self.assertEqual(len(reports['1h']['threads'][0]['tool_events']), 5)
        self.assertEqual(len(reports['all']['threads'][0]['tool_events']), 100)
        for key in ('tokens', 'status', 'model'):
            self.assertEqual(reports['all']['threads'][0][key], reports['1h']['threads'][0][key])

    def test_dashboard_mcp_scope_and_latest_source_state(self):
        self.write()
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        recent = dashboard.snapshot('1h')
        all_data = dashboard.snapshot('all')
        self.assertEqual(recent['mcp']['servers'][0]['calls'], 5)
        self.assertEqual(all_data['mcp']['servers'][0]['calls'], 130)
        self.assertFalse(recent['mcp']['recording_status']['future']['enabled'])
        self.assertEqual(recent['mcp']['servers'][0]['connection'], all_data['mcp']['servers'][0]['connection'])
        self.assertEqual(recent['sources']['session']['display_window'], '1h')
        self.assertEqual(recent['sources']['session']['limits']['file_limit'], all_data['sources']['session']['limits']['file_limit'])

    def test_log_window_filters_before_display_limit(self):
        dashboard = Dashboard(self.home)
        old = (self.reference-timedelta(days=2)).isoformat()
        recent = (self.reference-timedelta(minutes=5)).isoformat()
        dashboard.diagnostics.logs.extend([{'timestamp': old, 'kind': 'old', 'index': index} for index in range(2100)])
        dashboard.diagnostics.logs.append({'timestamp': recent, 'kind': 'new'})
        dashboard.observations['codex'] = True
        self.assertTrue(any(item.get('kind') == 'new' for item in dashboard.logs('1h')['entries']))
        self.assertFalse(any(item.get('kind') == 'old' for item in dashboard.logs('1h')['entries']))

    def test_latest_metrics_precede_table_cap_and_keep_false_zero(self):
        rare = {'server': 'rare', 'tool': 'status', 'nested': False, 'timestamp': '2026-10-05T00:00:00Z',
                'result': {'items_count': 0, 'recording_enabled': False, 'unsafe': 'PRIVATE', 'invalid_ms': float('inf')}}
        busy = rare | {'server': 'busy', 'timestamp': '2026-10-05T00:01:00Z', 'result': {}}
        report = summarize({}, [rare]+[busy]*1001)
        source = next(item for item in report['servers'] if item['server'] == 'rare')
        self.assertEqual(source['latest_metrics']['items_count']['value'], 0)
        self.assertFalse(source['latest_metrics']['recording_enabled']['value'])
        self.assertEqual(set(source['latest_metrics']), {'items_count', 'recording_enabled'})
        self.assertTrue(all(item['server'] == 'busy' for item in report['events']))

    def test_skill_counts_use_full_retained_calls_in_each_window(self):
        self.write()
        collector = CodexCollector(self.root)
        collector.refresh()
        state = next(iter(collector.files.values()))
        original = next(iter(state['calls'].values()))
        state['calls'] = {}
        for index in range(600):
            when = self.reference-timedelta(days=2) if index < 50 else self.reference-timedelta(minutes=5)
            state['calls'][str(index)] = original | {'timestamp': when.isoformat(), 'completed_at': None,
                                                    'skills': [{'skill': 'demo'}], 'mcp': []}
        reports = collector.snapshot_windows()
        self.assertEqual(reports['all']['skills']['counts']['demo'], 600)
        self.assertEqual(reports['all']['skills']['total'], 600)
        self.assertEqual(reports['1h']['skills']['counts']['demo'], 550)
        self.assertEqual(reports['1h']['skills']['total'], 550)
        self.assertEqual(len(reports['1h']['skills']['events']), 500)
        self.assertEqual(len(reports['all']['skills']['events']), 500)

    def test_dot_events_filter_before_cap_and_unknown_is_all_only(self):
        self.write()
        thread = '00000000-0000-0000-0000-000000000001'
        old_ms = (self.reference-timedelta(days=2)).timestamp()*1000
        recent_ms = (self.reference-timedelta(minutes=5)).timestamp()*1000
        outputs = [{'threadId': thread, 'turnId': str(index), 'artifact': {'producedAtMs': old_ms, 'type': 'file'}} for index in range(501)]
        outputs += [{'threadId': thread, 'turnId': 'recent', 'artifact': {'producedAtMs': recent_ms, 'type': 'file'}},
                    {'threadId': thread, 'turnId': 'unknown', 'artifact': {'type': 'file'}}]
        state = {'electron-persisted-atom-state': {'orbit-outputs-v1': outputs,
                 'orbit-activity-snapshots-v1': [{'data': [1, 2, 3]}]}}
        (self.home/'.codex-global-state.json').write_text(json.dumps(state), encoding='utf-8')
        collector = CodexCollector(self.root)
        collector.refresh()
        reports = collector.snapshot_windows()
        self.assertEqual(reports['all']['dots']['total'], 503)
        self.assertEqual(len(reports['all']['dots']['events']), 500)
        self.assertEqual(reports['1h']['dots']['total'], 1)
        self.assertEqual(len(reports['1h']['dots']['events']), 1)
        self.assertEqual(reports['all']['dots']['activity_items'], 3)
        self.assertEqual(reports['1h']['dots']['activity_items_scope'], 'latest_loaded_snapshot')
        self.assertNotIn('_retained_events', reports['all']['dots'])
        unknown = collector.snapshot('all')['dots']
        self.assertEqual(unknown['total'], 503)

    def test_monitor_window_filters_samples_and_preserves_latest_scalars(self):
        dashboard = Dashboard(self.home)
        old = (self.reference-timedelta(days=2)).isoformat()
        recent = (self.reference-timedelta(minutes=5)).isoformat()
        dashboard.monitor.history.extend([
            {'time': old, 'refresh_ms': 900, 'cpu_ms': 90},
            {'refresh_ms': 800, 'cpu_ms': 80},
            {'time': recent, 'refresh_ms': 0, 'cpu_ms': 0}])
        dashboard.monitor.events.clear()
        dashboard.monitor.events.extend([
            {'timestamp': old, 'kind': 'settings_applied'},
            {'kind': 'settings_applied'},
            {'timestamp': recent, 'kind': 'settings_applied'}])
        dashboard.monitor.refreshes = 17
        dashboard.monitor.requests = 9
        dashboard.monitor.errors = 3
        with patch.object(Path, 'open', side_effect=AssertionError('unexpected source read')):
            all_data = dashboard.snapshot('all')['monitor']
            recent_data = dashboard.snapshot('1h')['monitor']
        self.assertEqual(len(all_data['history']), 3)
        self.assertEqual(len(all_data['events']), 3)
        self.assertEqual(len(recent_data['history']), 1)
        self.assertEqual(len(recent_data['events']), 1)
        self.assertEqual(recent_data['history'][0]['refresh_ms'], 0)
        self.assertEqual(recent_data['window'], '1h')
        self.assertEqual(recent_data['unknown_timestamp'], 'all_only')
        for key in ('refreshes', 'requests', 'errors', 'refresh_ms', 'cpu_ms', 'python', 'version', 'pid'):
            self.assertEqual(recent_data[key], all_data[key])
        self.assertEqual(recent_data['refreshes'], 17)


if __name__ == '__main__':
    unittest.main()
