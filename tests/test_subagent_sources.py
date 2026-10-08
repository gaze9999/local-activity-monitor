import json
import os
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest

from local_activity_monitor.codex_metadata import read_metadata, spawn_metadata
from local_activity_monitor.collectors import CodexCollector

PARENT = '00000000-0000-4000-8000-000000000001'
CHILD = '00000000-0000-4000-8000-000000000002'
OTHER = '00000000-0000-4000-8000-000000000003'


class SubagentSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name).resolve()
        with closing(sqlite3.connect(self.home/'state_5.sqlite')) as db, db:
            db.execute('CREATE TABLE threads(id TEXT,title TEXT,source TEXT,agent_nickname TEXT,agent_path TEXT,created_at_ms INTEGER,updated_at_ms INTEGER,thread_source TEXT,first_user_message TEXT)')
            db.execute('CREATE TABLE thread_spawn_edges(parent_thread_id TEXT,child_thread_id TEXT,status TEXT)')
            for identity, title, parent in ((PARENT,'Parent',None),(CHILD,'Worker',PARENT),(OTHER,'Unselected',None)):
                source = json.dumps({'subagent':{'thread_spawn':{'parent_thread_id':parent,'agent_nickname':'Tesla','agent_path':'/root/worker'}}}) if parent else 'vscode'
                db.execute('INSERT INTO threads VALUES(?,?,?,?,?,?,?,?,?)',(identity,title,source,'Tesla' if parent else None,'/root/worker' if parent else None,1791127657099,1791128207322,'subagent' if parent else 'user','PRIVATE_PROMPT'))
            db.execute('INSERT INTO thread_spawn_edges VALUES(?,?,?)',(PARENT,CHILD,'open'))
            db.execute('INSERT INTO thread_spawn_edges VALUES(?,?,?)',(OTHER,OTHER,'open'))

    def test_edges_discover_only_selected_relations_and_project_source_fields(self):
        report = {}
        entries = read_metadata(self.home, {PARENT}, source_info=report)
        self.assertIn(CHILD, entries)
        self.assertNotIn(OTHER, entries)
        child = entries[CHILD]
        self.assertEqual(child['execution']['parent_thread_id'], PARENT)
        self.assertEqual(child['execution']['agent_path'], '/root/worker')
        self.assertEqual(child['execution']['agent_nickname'], 'Tesla')
        self.assertEqual(child['trigger'], 'subagent')
        self.assertEqual(child['created_at'], '2026-10-04T15:27:37.099Z')
        self.assertNotIn('status', child)
        self.assertNotIn('PRIVATE_PROMPT', json.dumps([entries,report]))
        self.assertEqual(report[str(self.home/'state_5.sqlite')]['subagent_rows'],1)
        self.assertEqual(spawn_metadata('{"subagent":{"thread_spawn":{"agent_path":"C:/private","parent_thread_id":"invalid"}}}'),{})

    def test_related_tail_status_is_cached_and_budgeted_without_content(self):
        sessions = self.home/'sessions'
        sessions.mkdir()
        parent = sessions/('rollout-'+PARENT+'.jsonl')
        child = sessions/('rollout-'+CHILD+'.jsonl')
        parent.write_text(json.dumps({'type':'session_meta','payload':{'id':PARENT}})+'\n',encoding='utf-8')
        event = {'type':'event_msg','timestamp':'2026-10-04T15:00:00Z','payload':{'type':'task_complete','last_agent_message':'PRIVATE_RESPONSE'}}
        child.write_text('x'*70000+'\n'+json.dumps(event)+'\n',encoding='utf-8')
        os.utime(child,(100,100))
        collector = CodexCollector(sessions,max_files=1)
        collector.refresh()
        # Exercise the fallback for a related session absent from the loaded set.
        collector.files.pop(child,None)
        first = collector.snapshot()
        row = next(row for row in first['threads'] if row['thread_id']==CHILD)
        self.assertTrue(row['metadata_only'])
        self.assertEqual((row['status'],row['status_source']),('completed','session_lifecycle'))
        self.assertNotIn('PRIVATE_RESPONSE',json.dumps(first))
        report = next(info for info in first['metadata_sources'] if info['name']=='subagent_lifecycle')
        self.assertEqual(report['bytes_read'],collector.SUBAGENT_TAIL_LIMIT)
        self.assertEqual(next(info for info in collector.snapshot()['metadata_sources'] if info['name']=='subagent_lifecycle')['bytes_read'],0)
        child.write_text(json.dumps(event | {'timestamp':'2026-10-04T16:00:00Z','payload':{'type':'task_started'}})+'\n',encoding='utf-8')
        collector.related_budget = 0
        pending = collector.snapshot()
        self.assertEqual(next(info for info in pending['metadata_sources'] if info['name']=='subagent_lifecycle')['pending_files'],1)
        collector.related_budget = collector.READ_LIMIT
        self.assertEqual(next(row for row in collector.snapshot()['threads'] if row['thread_id']==CHILD)['status'],'running')
        child.unlink()
        self.assertEqual(next(info for info in collector.snapshot()['metadata_sources'] if info['name']=='subagent_lifecycle')['health'],'partly_unavailable')
