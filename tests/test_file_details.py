import json
from datetime import datetime, timezone
from pathlib import Path
import tempfile
import sys
import unittest

from local_activity_monitor.operation_records import file_operations
from local_activity_monitor.server import Dashboard


class FileDetailsTests(unittest.TestCase):
    def test_literal_ranges_and_submitted_text_size(self):
        values = file_operations([('read_file', {'path': 'a.txt', 'start_line': 0, 'end_line': 20, 'limit': True}, False)])
        self.assertEqual(values[0]['range'], {'start_line': 0, 'end_line': 20})
        value = file_operations([('write_file', {'path': 'a.txt', 'content': '繁中'}, False)])[0]
        self.assertEqual(value['submitted_utf8_bytes'], 6)
        self.assertNotIn('繁中', json.dumps(value))
        value = file_operations([('exec_command', {'cmd': 'head -c 20 a.txt'}, True)])[0]
        self.assertEqual(value['range'], {'first_bytes': 20})
        self.assertEqual(file_operations([('exec_command', {'cmd': "sed -n '3,9p' a.txt"}, False)])[0]['range'], {'start_line': 3, 'end_line': 9})

    def test_actual_counts_only_for_one_isolated_file_and_preserve_zero(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            root = home/'sessions';root.mkdir()
            thread = '00000000-0000-0000-0000-000000000001'
            records = [('session_meta', {'id':thread}), ('response_item', {'type':'function_call','name':'read_file','call_id':'one','arguments':'{"path":"a.txt"}'}), ('response_item', {'type':'function_call_output','call_id':'one','output':'{"bytes_read":0}'}), ('response_item', {'type':'custom_tool_call','name':'exec','call_id':'two','input':'await tools.read_file({path:"b.txt"}); await tools.read_file({path:"c.txt"});'}), ('response_item', {'type':'custom_tool_call_output','call_id':'two','output':'{"bytes_read":99}'})]
            timestamp = datetime.now(timezone.utc).isoformat()
            root.joinpath('rollout-'+thread+'.jsonl').write_text('\n'.join(json.dumps({'type':kind,'timestamp':timestamp,'payload':payload}) for kind,payload in records)+'\n', encoding='utf-8')
            app = Dashboard(home, True);app.refresh()
            events = app.snapshot('all')['codex']['file_activity']['events']
            self.assertEqual(next(item for item in events if item['call_id']=='one')['read_bytes'], 0)
            self.assertTrue(all('read_bytes' not in item for item in events if item['call_id']=='two'))

    def test_current_size_requires_observed_path_and_is_bounded(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            if sys.platform == 'darwin':
                home = home.resolve()
            file = home/'a.txt';file.write_bytes(b'1234')
            app = Dashboard(home, True)
            event = {'thread_id':'1'*36, 'call_id':'call1', 'path':str(file)}
            app.cache = {'all':{'codex':{'file_activity':{'events':[event]*200}}}}
            result = app.file_summary('all')
            self.assertEqual(len(result['files']), 1)
            self.assertEqual(result['files'][0]['bytes'], 4)
            self.assertIsNone(app.file_detail('1'*36, 'call1', str(home/'other')))
            app.observations['files'] = False
            self.assertIsNone(app.file_summary('all'))

    def test_current_sizes_stop_at_100_observed_positions_and_use_window(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            if sys.platform == 'darwin':
                home = home.resolve()
            events = []
            for index in range(105):
                path = home/(str(index)+'.txt')
                path.write_bytes(b'1234')
                events.append({'thread_id':'1'*36, 'call_id':'call'+str(index), 'path':str(path)})
            app = Dashboard(home, True)
            app.cache = {'all':{'codex':{'file_activity':{'events':events}}}, '1h':{'codex':{'file_activity':{'events':events[-1:]}}}}
            result = app.file_summary('all')
            self.assertEqual(result['total'], 105)
            self.assertEqual(len(result['files']), 100)
            self.assertEqual(app.file_summary('1h')['files'][0]['path'], events[-1]['path'])
