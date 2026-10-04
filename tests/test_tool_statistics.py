import json
from pathlib import Path
import tempfile
import unittest

from local_activity_monitor.collectors import CodexCollector


class ToolStatisticsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)/'sessions'
        self.root.mkdir()

    def write(self, index, calls):
        thread = f'00000000-0000-0000-0000-{index:012d}'
        records = [{'type': 'session_meta', 'timestamp': '2026-10-05T00:00:00Z', 'payload': {'id': thread}}]
        for identity, tool, start, end, arguments in calls:
            records.append({'type': 'response_item', 'timestamp': start, 'payload': {
                'type': 'function_call', 'name': tool, 'call_id': identity, 'arguments': arguments}})
            if end:
                records.append({'type': 'response_item', 'timestamp': end, 'payload': {
                    'type': 'function_call_output', 'call_id': identity, 'output': 'PRIVATE_RESULT'}})
        (self.root/f'rollout-{thread}.jsonl').write_text('\n'.join(map(json.dumps, records))+'\n', encoding='utf-8')

    def snapshot(self, **features):
        collector = CodexCollector(self.root)
        collector.features.update(features)
        collector.refresh()
        return collector.snapshot()

    def test_statistics_use_all_retained_calls_before_event_cap(self):
        calls = [(str(i), 'future_tool', '2026-10-05T00:00:00Z', '2026-10-05T00:00:01Z', '{}') for i in range(130)]
        self.write(1, calls)
        self.write(2, [('other', 'future_tool', '2026-10-05T00:00:01Z', None, '{}')])
        data = self.snapshot()
        item = data['tool_statistics'][0]
        self.assertEqual(item['calls'], 131)
        self.assertEqual(item['thread_count'], 2)
        self.assertEqual(item['returned'], 130)
        self.assertEqual(item['known_duration_count'], 130)
        self.assertEqual(item['average_ms'], 1000)
        self.assertEqual(item['p99_ms'], 1000)
        self.assertEqual(data['tools']['future_tool'], item['calls'])
        self.assertEqual(max(len(row['tool_events']) for row in data['threads']), 100)
        self.assertNotIn('PRIVATE_RESULT', json.dumps(data))

    def test_zero_duration_and_unknown_are_distinct(self):
        self.write(1, [('zero', 'zero_tool', '2026-10-05T00:00:00Z', '2026-10-05T00:00:00Z', '{}'),
                       ('pending', 'zero_tool', '2026-10-05T00:00:02Z', None, '{}'),
                       ('unknown', 'future_tool', '2026-10-05T00:00:01Z', None, '{}')])
        rows = {item['tool']: item for item in self.snapshot()['tool_statistics']}
        self.assertEqual(rows['zero_tool']['average_ms'], 0)
        self.assertEqual(rows['zero_tool']['p99_ms'], 0)
        self.assertEqual(rows['zero_tool']['known_duration_count'], 1)
        self.assertEqual(rows['zero_tool']['last_at'], '2026-10-05T00:00:02.000Z')
        self.assertEqual(rows['zero_tool']['last_response_at'], '2026-10-05T00:00:00.000Z')
        self.assertEqual(rows['future_tool']['returned'], 0)
        self.assertEqual(rows['future_tool']['known_duration_count'], 0)
        self.assertIsNone(rows['future_tool']['average_ms'])

    def test_nested_counts_do_not_inherit_container_duration_or_response(self):
        args = 'await tools.future_tool({}); await tools.future_tool({});'
        self.write(1, [('exec', 'exec', '2026-10-05T00:00:00Z', '2026-10-05T00:00:01Z', args)])
        data = self.snapshot()
        nested = next(item for item in data['tool_statistics'] if item['nested'])
        self.assertEqual(nested['calls'], 2)
        self.assertEqual(nested['calls'], data['nested_tools']['future_tool'])
        self.assertEqual(nested['thread_count'], 1)
        for key in ('returned', 'known_duration_count', 'average_ms', 'p99_ms', 'last_response_at'):
            self.assertIsNone(nested[key])

    def test_feature_disabled_has_no_new_statistics(self):
        self.write(1, [('call', 'future_tool', '2026-10-05T00:00:00Z', None, '{}')])
        data = self.snapshot(tool_events=False)
        self.assertEqual(data['tool_statistics'], [])
        self.assertEqual(data['tools']['future_tool'], 1)

    def test_duration_percentile_uses_only_measured_calls(self):
        self.write(1, [('fast', 'future_tool', '2026-10-05T00:00:00Z', '2026-10-05T00:00:00Z', '{}'),
                       ('slow', 'future_tool', '2026-10-05T00:00:01Z', '2026-10-05T00:00:02Z', '{}'),
                       ('pending', 'future_tool', '2026-10-05T00:00:03Z', None, '{}')])
        item = self.snapshot()['tool_statistics'][0]
        self.assertEqual(item['known_duration_count'], 2)
        self.assertEqual(item['average_ms'], 500)
        self.assertEqual(item['p99_ms'], 990)


if __name__ == '__main__':
    unittest.main()
