import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.server import Dashboard, handler
from local_activity_monitor.project_instructions import instructions, READ_LIMIT

THREAD = '00000000-0000-0000-0000-000000000001'


class ProjectTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        # macOS exposes its temp directory through /var -> /private/var.
        # Use real fixture paths while preserving rejection of linked roots.
        fixture_root = Path(self.temp.name).resolve()
        self.home = fixture_root/'codex'
        self.home.mkdir()
        sessions = self.home/'sessions'
        sessions.mkdir()
        (sessions/f'rollout-{THREAD}.jsonl').write_text(json.dumps({'type': 'session_meta', 'timestamp': '2026-10-05T00:00:00Z', 'payload': {'id': THREAD}})+'\n', encoding='utf-8')
        self.root = fixture_root/'PRIVATE_WORKSPACE_ROOT'
        self.root.mkdir()

    def database(self, archived=1):
        with closing(sqlite3.connect(self.home/'state_5.sqlite')) as db:
            db.executescript('CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT,archived); CREATE TABLE projects(id TEXT,name TEXT); CREATE TABLE project_roots(project_id TEXT,path TEXT);')
            db.execute('INSERT INTO threads VALUES(?,?,?,?)', (THREAD, 'Thread', 'p1', archived))
            db.executemany('INSERT INTO projects VALUES(?,?)', [('p1', 'Project One'), ('unloaded', 'Unloaded Project')])
            db.executemany('INSERT INTO project_roots VALUES(?,?)', [('p1', str(self.root)), ('unloaded', '/PRIVATE_UNLOADED_ROOT')])
            db.commit()

    def test_selected_database_project_and_archived_without_snapshot_folders(self):
        self.database()
        (self.root/'AGENTS.md').write_text('PRIVATE_INSTRUCTION_BODY', encoding='utf-8')
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        snapshot = dashboard.snapshot('all')
        self.assertEqual(snapshot['codex']['projects'][0]['name'], 'Project One')
        self.assertEqual(snapshot['codex']['projects'][0]['thread_count'], 1)
        self.assertTrue(snapshot['codex']['threads'][0]['archived'])
        self.assertIsNone(snapshot['codex']['projects'][0]['kind'])
        encoded = json.dumps(snapshot)
        self.assertNotIn('PRIVATE_WORKSPACE_ROOT', encoded)
        self.assertNotIn('PRIVATE_INSTRUCTION_BODY', encoded)
        self.assertNotIn('PRIVATE_UNLOADED_ROOT', encoded)
        self.assertEqual(dashboard.project_detail('unloaded')['thread_count'], 0)
        detail = dashboard.project_detail('p1')
        self.assertEqual(detail['folders'], [str(self.root)])
        self.assertEqual(detail['threads'][0]['thread_id'], THREAD)
        self.assertNotIn('environment', snapshot['codex']['projects'][0])

    def test_legacy_project_kind_and_roots_are_explicit(self):
        self.database(0)
        state = {'local-projects': {'legacy': {'id': 'legacy', 'name': 'Legacy', 'rootPaths': [str(self.root)]}},
                 'thread-project-assignments': {THREAD: {'projectId': 'legacy', 'projectKind': 'cloud', 'projectOrigin': 'explicit'}}}
        (self.home/'.codex-global-state.json').write_text(json.dumps(state), encoding='utf-8')
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        project = next(item for item in dashboard.snapshot('all')['codex']['projects'] if item['id'] == 'legacy')
        self.assertEqual(project['id'], 'legacy')
        self.assertEqual(project['kind'], 'cloud')
        self.assertFalse(dashboard.snapshot('all')['codex']['threads'][0]['archived'])
        self.assertEqual(dashboard.project_detail('legacy')['folders'], [str(self.root)])

    def test_unknown_archive_value_is_not_true_or_false(self):
        self.database('false')
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        self.assertIsNone(dashboard.snapshot('all')['codex']['threads'][0]['archived'])

    def test_legacy_mapping_requires_existing_target_and_exact_host(self):
        self.database()
        state = {'local-projects': {'legacy': {'name': 'Legacy', 'rootPaths': [str(self.root)]}},
                 'thread-project-assignments': {THREAD: {'projectId': 'legacy', 'projectKind': 'local'}},
                 'app-server-project-id-by-legacy-project-id-by-host': {'local': {'legacy': 'p1'}}}
        path = self.home/'.codex-global-state.json'
        path.write_text(json.dumps(state), encoding='utf-8')
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        data = dashboard.snapshot('all')['codex']
        self.assertEqual(data['threads'][0]['project_id'], 'p1')
        self.assertEqual(data['threads'][0]['project_name'], 'Project One')
        self.assertNotIn('legacy', {item['id'] for item in data['projects']})
        state['app-server-project-id-by-legacy-project-id-by-host']['local']['legacy'] = 'nonexistent'
        path.write_text(json.dumps(state), encoding='utf-8')
        dashboard.refresh()
        self.assertEqual(dashboard.snapshot('all')['codex']['threads'][0]['project_id'], 'legacy')
        state['app-server-project-id-by-legacy-project-id-by-host'] = {'other-host': {'legacy': 'p1'}}
        path.write_text(json.dumps(state), encoding='utf-8')
        dashboard.refresh()
        self.assertEqual(dashboard.snapshot('all')['codex']['threads'][0]['project_id'], 'legacy')

    def test_project_source_counts_do_not_mix_database_and_legacy(self):
        self.database()
        (self.home/'.codex-global-state.json').write_text(json.dumps({'local-projects': {}}), encoding='utf-8')
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        reports = {item['name']: item for item in dashboard.snapshot('all')['codex']['metadata_sources']}
        self.assertEqual(reports['state_5.sqlite']['project_rows'], 2)
        self.assertEqual(reports['.codex-global-state.json']['loaded_projects'], 0)

    def test_same_project_id_preserves_database_name_and_root(self):
        self.database()
        state = {'local-projects': {'p1': {'name': 'Outdated Legacy Name', 'rootPaths': ['/PRIVATE_LEGACY_ROOT']}},
                 'thread-project-assignments': {THREAD: {'projectId': 'p1'}}}
        (self.home/'.codex-global-state.json').write_text(json.dumps(state), encoding='utf-8')
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        data = dashboard.snapshot('all')['codex']
        project = next(item for item in data['projects'] if item['id'] == 'p1')
        self.assertEqual(project['name'], 'Project One')
        self.assertEqual(data['threads'][0]['project_name'], 'Project One')
        detail = dashboard.project_detail('p1')
        self.assertEqual(detail['source'], 'projects')
        self.assertEqual(detail['folders'], [str(self.root)])
        reports = {item['name']: item for item in data['metadata_sources']}
        self.assertEqual(reports['state_5.sqlite']['project_rows'], 2)
        self.assertEqual(reports['.codex-global-state.json']['loaded_projects'], 1)

    def test_instructions_are_on_demand_bounded_and_redacted(self):
        self.database()
        body = 'api_key=PRIVATE_CREDENTIAL\n'+'TEXT'*20000
        (self.root/'AGENTS.md').write_text(body, encoding='utf-8')
        (self.home/'AGENTS.md').write_text('Global instructions', encoding='utf-8')
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        result = dashboard.instruction_detail('project', 'p1')
        self.assertEqual(len(result['documents']), 1)
        document = result['documents'][0]
        self.assertEqual(document['read_bytes'], READ_LIMIT)
        self.assertTrue(document['truncated'])
        self.assertNotIn('PRIVATE_CREDENTIAL', document['text'])
        self.assertEqual(dashboard.instruction_detail('global')['documents'][0]['text'], 'Global instructions')
        self.assertIsNone(dashboard.instruction_detail('unknown'))
        self.assertIsNone(dashboard.instruction_detail('project', 'unknown'))
        self.assertIsNone(dashboard.instruction_detail('global', 'p1'))
        self.assertNotIn('TEXTTEXT', json.dumps(dashboard.snapshot('all')))
        for path in (self.home/'monitoring').glob('*'):
            if path.is_file():
                self.assertNotIn('TEXTTEXT', path.read_text(encoding='utf-8'))

    def test_missing_and_protected_instruction_roots(self):
        self.assertEqual(instructions([self.root])['documents'], [])
        with patch.object(Path, 'is_symlink', return_value=True), patch.object(Path, 'open', side_effect=AssertionError('unexpected read')):
            self.assertEqual(instructions([self.root])['documents'][0]['health'], 'rejected')
        self.assertEqual(instructions(['//network/share'])['documents'][0]['health'], 'rejected')

    def test_http_project_and_instruction_queries_reject_arbitrary_paths(self):
        self.database()
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        request_handler = handler(dashboard, 8787)
        def request(query):
            request = object.__new__(request_handler)
            request.headers = {'Host': '127.0.0.1:8787'}
            request.path = query
            result = []
            request.reply = lambda code, body, *args, **kwargs: result.append((code, body))
            request.do_GET()
            return result[0]
        for query in ('/api/codex/project?project_id=../outside',
                          '/api/codex/project?project_id=p1&path=anything',
                          '/api/codex/instructions?scope=global&project_id=p1',
                          '/api/codex/instructions?scope=project&project_id=p1&file=auth.json',
                          '/api/codex/instructions?scope=other'):
            with self.subTest(query=query):
                self.assertEqual(request(query)[0], 400)
        code, body = request('/api/codex/project?project_id=p1')
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)['folders'], [str(self.root)])
        self.assertEqual(request('/api/codex/project?project_id=unknown')[0], 409)


if __name__ == '__main__':
    unittest.main()
