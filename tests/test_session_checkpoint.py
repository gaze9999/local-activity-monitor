"""Metadata-only restart, partial-line replay and replacement checks."""
from contextlib import closing
from datetime import datetime, timezone
import json
from pathlib import Path
import tempfile
import unittest

from local_activity_monitor.server import Dashboard


class SessionCheckpointTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.sessions = self.home/'sessions'
        self.sessions.mkdir()
        self.identity = '00000000-0000-4000-8000-000000000001'
        self.path = self.sessions/('rollout-'+self.identity+'.jsonl')
        self.stamp = datetime.now(timezone.utc).isoformat()

    def record(self, kind, payload):
        return json.dumps({'timestamp':self.stamp,'type':kind,'payload':payload})+'\n'

    def seed(self):
        self.path.write_text(self.record('session_meta',{'id':self.identity})+
                             self.record('turn_context',{'model':'demo-model','reasoning_effort':'high'})+
                             self.record('event_msg',{'type':'token_count','info':{'total_token_usage':{'input_tokens':12}}})+
                             self.record('response_item',{'type':'function_call','name':'mcp__future__run','call_id':'call1','arguments':json.dumps({'message':'PRIVATE_BODY'})}),encoding='utf-8')

    def test_resume_without_replaying_old_lines_and_complete_pending_call(self):
        self.seed()
        first = Dashboard(self.home,codex=True)
        first.codex.refresh()
        checkpoint = first.codex.session_checkpoint
        with closing(checkpoint.store.connect()) as db:
            payloads = [row[0] for row in db.execute('SELECT payload FROM session_cursors UNION ALL SELECT payload FROM session_calls')]
        self.assertNotIn('PRIVATE_BODY',''.join(payloads))
        second = Dashboard(self.home,codex=True)
        second.codex.refresh()
        state = second.codex.files[self.path]
        self.assertEqual(second.codex.read_bytes,0)
        self.assertEqual(state['model'],'demo-model')
        self.assertEqual(state['tokens']['input_tokens'],12)
        self.assertIn('call1',state['calls'])
        appended = self.record('response_item',{'type':'function_call_output','call_id':'call1','output':'PRIVATE_RESPONSE'})
        with self.path.open('a',encoding='utf-8',newline='\n') as stream:
            stream.write(appended)
        second.codex.refresh()
        self.assertEqual(second.codex.read_bytes,len(appended.encode()))
        self.assertIsNotNone(state['calls']['call1']['completed_at'])
        self.assertEqual(second.activity_history.store.page('mcp')['total'],1)

    def test_partial_line_replays_only_uncommitted_suffix_after_restart(self):
        self.seed()
        partial = self.record('event_msg',{'type':'token_count','info':{'total_token_usage':{'input_tokens':123}}})
        with self.path.open('a',encoding='utf-8',newline='\n') as stream:
            stream.write(partial[:len(partial)//2])
        first = Dashboard(self.home,codex=True)
        first.codex.refresh()
        with self.path.open('a',encoding='utf-8',newline='\n') as stream:
            stream.write(partial[len(partial)//2:])
        second = Dashboard(self.home,codex=True)
        second.codex.refresh()
        self.assertEqual(second.codex.files[self.path]['tokens']['input_tokens'],123)
        self.assertEqual(second.codex.read_bytes,len(partial.encode()))

    def test_revision_changes_only_with_session_state(self):
        self.seed()
        dashboard = Dashboard(self.home,codex=True)
        dashboard.codex.refresh()
        revision = dashboard.codex.revision
        dashboard.codex.next_scan = 0
        dashboard.codex.refresh()
        self.assertEqual(dashboard.codex.revision,revision)
        with self.path.open('a',encoding='utf-8',newline='\n') as stream:
            stream.write(self.record('response_item',{'type':'function_call_output','call_id':'call1','output':'{}'}))
        dashboard.codex.refresh()
        self.assertGreater(dashboard.codex.revision,revision)

    def test_projection_reuses_records_only_for_unchanged_threads(self):
        self.seed()
        other = '00000000-0000-4000-8000-000000000002'
        (self.sessions/('rollout-'+other+'.jsonl')).write_text(json.dumps({'timestamp':self.stamp,'type':'session_meta','payload':{'id':other}})+'\n'+
            self.record('response_item',{'type':'function_call','name':'exec_command','call_id':'call2','arguments':json.dumps({'cmd':'git status'})}),encoding='utf-8')
        dashboard = Dashboard(self.home,codex=True)
        dashboard.codex.refresh()
        dashboard.codex.snapshot_windows()
        first = dict(dashboard.codex.derived_calls)
        dashboard.codex.snapshot_windows()
        self.assertIs(dashboard.codex.derived_calls[self.identity][1],first[self.identity][1])
        with self.path.open('a',encoding='utf-8',newline='\n') as stream:
            stream.write(self.record('response_item',{'type':'function_call_output','call_id':'call1','output':'{}'}))
        dashboard.codex.refresh()
        projection = dashboard.codex.snapshot_windows()['all']
        self.assertIsNot(dashboard.codex.derived_calls[self.identity][1],first[self.identity][1])
        self.assertIs(dashboard.codex.derived_calls[other][1],first[other][1])
        row = next(row for row in projection['threads'] if row['thread_id']==self.identity)
        self.assertIsNotNone(row['tool_events'][0]['completed_at'])

    def test_replaced_or_rewritten_source_invalidates_checkpoint(self):
        self.seed()
        first = Dashboard(self.home,codex=True)
        first.codex.refresh()
        self.path.write_text(self.path.read_text(encoding='utf-8').replace('demo-model','other-model'),encoding='utf-8')
        second = Dashboard(self.home,codex=True)
        second.codex.refresh()
        self.assertEqual(second.codex.files[self.path]['model'],'other-model')
        self.assertGreater(second.codex.read_bytes,0)


if __name__=='__main__':
    unittest.main()
