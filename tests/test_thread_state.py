import json
from datetime import datetime, timezone
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.thread_state import ThreadState


class ThreadStateTests(unittest.TestCase):
    def test_unchanged_state_skips_writes_but_changed_offset_saves_and_retries(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = ThreadState(Path(directory)/'thread-state.json')
            stamp = datetime.now(timezone.utc).isoformat()
            key = 'rollout-example.jsonl'
            state = {'thread_id':'00000000-0000-4000-8000-000000000001', 'source_file':key,
                'status':'running', 'offset':100, 'task_time':stamp, 'task_start':stamp, 'calls':{}}
            cache.update([state])
            checked = cache.write_checked_at
            with patch.object(cache.store, 'save', side_effect=AssertionError('Unchanged state should not save')):
                cache.update([state])
            self.assertEqual(cache.write_checked_at, checked)
            changed = state|{'offset':200}
            with patch.object(cache.store, 'save', side_effect=PermissionError):
                cache.update([changed])
            self.assertEqual(cache.write_health, 'unavailable')
            self.assertEqual(cache.store.read_document(cache.BYTE_LIMIT)['entries'][key]['offset'], 100)
            cache.update([changed])
            self.assertEqual(cache.write_health, 'ok')
            self.assertEqual(cache.store.read_document(cache.BYTE_LIMIT)['entries'][key]['offset'], 200)
            with patch.object(cache.store, 'save', side_effect=AssertionError('Successful retry should allow skipping')):
                cache.update([changed])

    def test_check_times_follow_load_and_failed_save_attempts(self):
        with tempfile.TemporaryDirectory() as directory, patch('local_activity_monitor.thread_state.datetime', wraps=datetime) as clock:
            clock.now.return_value = datetime(2026, 10, 9, 1, tzinfo=timezone.utc)
            cache = ThreadState(Path(directory)/'thread-state.json')
            self.assertEqual(cache.load_checked_at, '2026-10-09T01:00:00.000Z')
            self.assertIsNone(cache.write_checked_at)
            clock.now.return_value = datetime(2026, 10, 9, 2, tzinfo=timezone.utc)
            with patch.object(cache.store, 'save', side_effect=PermissionError):
                cache.update([])
            self.assertEqual(cache.write_health, 'unavailable')
            self.assertEqual(cache.write_checked_at, '2026-10-09T02:00:00.000Z')
            self.assertEqual(cache.load_checked_at, '2026-10-09T01:00:00.000Z')

    def test_checkpoint_projects_metadata_and_tolerates_write_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'thread-state.json'
            identity = '00000000-0000-4000-8000-000000000001'
            entry = {'thread_id':identity, 'status':'running', 'offset':100, 'task_time':'2026-10-04T00:00:00Z', 'task_start':'2026-10-04T00:00:00Z', 'command':'PRIVATE'}
            key = f'rollout-{identity}.jsonl'
            skill = {'thread_id':identity, 'call_id':'read_1', 'skill':'example', 'timestamp':'2026-10-04T00:00:00Z', 'doc_path':'PRIVATE'}
            path.write_text(json.dumps({'version':1, 'entries':{key:entry, '../bad':entry}, 'skills':[skill, {**skill, 'timestamp':'invalid'}]}))
            legacy = path.read_bytes()
            cache = ThreadState(path)
            self.assertEqual(list(cache.entries), [key])
            self.assertNotIn('command', cache.entries[key])
            self.assertEqual(len(cache.skills), 1)
            self.assertNotIn('doc_path', cache.skills[0])
            with patch.object(cache.store, 'save', side_effect=PermissionError):
                cache.update([{**entry, 'source_file':key, 'calls':{}}])
            self.assertEqual(cache.entries[key]['status'], 'running')
            cache.next_save = 0
            cache.update([{**entry, 'source_file':key, 'calls':{}}])
            self.assertNotIn('PRIVATE', json.dumps(cache.store.read_document(cache.BYTE_LIMIT)))
            self.assertEqual(path.read_bytes(), legacy)
            path.write_bytes(b' '*(ThreadState.BYTE_LIMIT+1))
            self.assertEqual(ThreadState(path).entries, cache.entries)


if __name__ == '__main__':
    unittest.main()
