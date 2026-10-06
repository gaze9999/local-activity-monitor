import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.monitor_state import MonitorState
from local_activity_monitor.server import Dashboard


class LazyCollectionTests(unittest.TestCase):
    def test_starting_snapshot_does_not_wait_for_collection_or_hardware(self):
        with tempfile.TemporaryDirectory() as folder, \
             patch('local_activity_monitor.monitor_state.gpu_info', return_value=[]) as gpu:
            dashboard=Dashboard(Path(folder),codex=True)
            gpu.assert_not_called()
            entered,release=threading.Event(),threading.Event()
            def slow_scan(*args,**kwargs):
                entered.set()
                release.wait(3)
            with patch.object(dashboard.codex,'refresh',side_effect=slow_scan):
                worker=threading.Thread(target=dashboard.refresh)
                worker.start()
                try:
                    self.assertTrue(entered.wait(1))
                    result=dashboard.snapshot('24h')
                    self.assertIsNone(result['updated_at'])
                    self.assertEqual(result['monitor']['collection']['phase'],'sessions')
                    self.assertFalse(result['monitor']['device_ready'])
                finally:
                    release.set()
                    worker.join(3)
            self.assertFalse(worker.is_alive())
            gpu.assert_called_once()
            self.assertTrue(dashboard.snapshot('24h')['monitor']['device_ready'])

    def test_cancel_during_pause_keeps_published_snapshot(self):
        with tempfile.TemporaryDirectory() as folder:
            dashboard=Dashboard(Path(folder))
            dashboard.refresh()
            updated=dashboard.snapshot('24h')['updated_at']
            dashboard.stop.set()
            dashboard.refresh(stagger=True)
            self.assertEqual(dashboard.snapshot('24h')['updated_at'],updated)
            self.assertEqual(dashboard.monitor.snapshot()['collection']['phase'],'stopped')

    def test_unchanged_assets_are_not_reread_and_revision_detects_edits(self):
        with tempfile.TemporaryDirectory() as folder:
            dashboard=Dashboard(Path(folder))
            revision=dashboard.web_revision()
            with patch.object(Path,'read_bytes',side_effect=AssertionError('unchanged assets read')):
                self.assertEqual(dashboard.web_revision(),revision)
            dashboard.asset_signature=None
            self.assertEqual(dashboard.web_revision(),revision)

    def test_live_reads_cannot_consume_entire_backfill_budget(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            collector=CodexCollector(root,tail_bytes=1024*1024)
            collector.READ_LIMIT=256*1024
            paths=[]
            for index in range(2):
                path=root/f'rollout-00000000-0000-4000-8000-{index:012d}.jsonl'
                line=json.dumps({'type':'response_item','payload':{'type':'message','content':'PRIVATE'*200},'timestamp':'2099-01-01T00:00:00Z'}).encode()+b'\n'
                path.write_bytes(line*400)
                state=collector.new_file(path)
                state.update(offset=path.stat().st_size,history_cursor=path.stat().st_size,error_cursor=path.stat().st_size,discard=False)
                collector.files[path]=state
                paths.append((path,line))
            collector.next_scan=float('inf')
            initial=[state['history_cursor'] for state in collector.files.values()]
            error_initial=[state['error_cursor'] for state in collector.files.values()]
            for _ in range(4):
                for path,line in paths:
                    with path.open('ab') as stream:stream.write(line*200)
                before=collector.read_bytes
                collector.refresh()
                self.assertLessEqual(collector.read_bytes-before,collector.READ_LIMIT)
            for index,state in enumerate(collector.files.values()):
                self.assertLess(state['history_cursor'] or 0,initial[index])
                self.assertLess(state['error_cursor'] or 0,error_initial[index])
            result=collector.snapshot()
            self.assertNotIn('PRIVATE',json.dumps(result))
            self.assertGreater(result['read_state']['history_pending_bytes'],0)

    def test_backfill_rotates_when_no_sessions_have_new_data(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            collector=CodexCollector(root)
            collector.READ_LIMIT=128*1024
            collector.features['errors']=False
            line=json.dumps({'type':'response_item','payload':{'type':'message','content':'PRIVATE'*200}}).encode()+b'\n'
            for index in range(7):
                path=root/f'rollout-00000000-0000-4000-8000-{index:012d}.jsonl'
                path.write_bytes(line*800)
                state=collector.new_file(path)
                state.update(offset=path.stat().st_size,history_cursor=path.stat().st_size,discard=False)
                collector.files[path]=state
            collector.next_scan=float('inf')
            initial=[state['history_cursor'] for state in collector.files.values()]
            for _ in range(7):
                before=collector.read_bytes
                collector.refresh()
                self.assertLessEqual(collector.read_bytes-before,collector.READ_LIMIT)
            for index,state in enumerate(collector.files.values()):
                self.assertLess(state['history_cursor'] or 0,initial[index])

    def test_chunk_pause_resumes_without_recounting_calls(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            path=root/'rollout-00000000-0000-4000-8000-000000000001.jsonl'
            records=[{'type':'response_item','payload':{'type':'function_call','name':'test','call_id':f'call_{index}'},'timestamp':'2099-01-01T00:00:00Z'} for index in range(4)]
            path.write_text(''.join(json.dumps(record)+'\n' for record in records),encoding='utf-8')
            collector=CodexCollector(root)
            pauses=[]
            collector.refresh(pause=lambda:pauses.append(True) or True)
            collector.refresh()
            self.assertTrue(pauses)
            self.assertEqual(collector.snapshot()['observed_tool_calls'],4)


if __name__=='__main__':
    unittest.main()
