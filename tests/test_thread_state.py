import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.thread_state import ThreadState


class ThreadStateTests(unittest.TestCase):
    def test_checkpoint_projects_metadata_and_tolerates_write_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'thread-state.json'
            identity = '00000000-0000-4000-8000-000000000001'
            entry = {'thread_id':identity, 'status':'running', 'offset':100, 'task_time':'2026-10-04T00:00:00Z', 'task_start':'2026-10-04T00:00:00Z', 'command':'PRIVATE'}
            key = f'rollout-{identity}.jsonl'
            skill = {'thread_id':identity, 'call_id':'read_1', 'skill':'example', 'timestamp':'2026-10-04T00:00:00Z', 'doc_path':'PRIVATE'}
            path.write_text(json.dumps({'version':1, 'entries':{key:entry, '../bad':entry}, 'skills':[skill, {**skill, 'timestamp':'invalid'}]}))
            cache = ThreadState(path)
            self.assertEqual(list(cache.entries), [key])
            self.assertNotIn('command', cache.entries[key])
            self.assertEqual(len(cache.skills), 1)
            self.assertNotIn('doc_path', cache.skills[0])
            with patch('local_activity_monitor.thread_state.os.replace', side_effect=PermissionError):
                cache.update([{**entry, 'source_file':key, 'calls':{}}])
            self.assertEqual(cache.entries[key]['status'], 'running')
            cache.next_save = 0
            cache.update([{**entry, 'source_file':key, 'calls':{}}])
            self.assertNotIn('PRIVATE', path.read_text())
            path.write_bytes(b' '*(ThreadState.BYTE_LIMIT+1))
            self.assertEqual(ThreadState(path).entries, {})


if __name__ == '__main__':
    unittest.main()
