from contextlib import closing
from datetime import datetime, timezone
import json
import sqlite3
import unittest
from urllib.parse import urlencode

from tests import test_content_details as fixtures
from local_activity_monitor.payload_detail import complete_payload, content_page, parse_text


THREAD = fixtures.THREAD


class ContentPageTests(unittest.TestCase):
    setUp = fixtures.ContentDetailsTests.setUp
    write = fixtures.ContentDetailsTests.write
    get = fixtures.ContentDetailsTests.get
    def pages(self, path, query, field):
        status, result = self.get(path+'?'+urlencode(query | {'lazy': '1'}))
        self.assertEqual(status, 200)
        page = result[field]
        chunks = [page['text']]
        while page['next'] is not None:
            status, page = self.get(path+'?'+urlencode(query | {'lazy': '1', 'field': field, 'offset': page['next'], 'revision': page['revision']}))
            self.assertEqual(status, 200)
            self.assertLessEqual(len(page['text']), 32768)
            chunks.append(page['text'])
        return ''.join(chunks), result

    def test_large_tool_output_complete_masked_and_revision_checked(self):
        self.write('response_item', {'type': 'custom_tool_call', 'name': 'exec', 'call_id': 'large', 'input': 'text(1)'})
        output = {'items': [{'index': i, 'text': 'line\n'*500} for i in range(120)], 'password': 'PRIVATE', 'last': 'FINAL_CONTENT'}
        self.write('response_item', {'type': 'custom_tool_call_output', 'call_id': 'large', 'output': json.dumps(output)})
        self.app.refresh()
        query = {'thread_id': THREAD, 'call_id': 'large'}
        text, initial = self.pages('/api/codex/tool', query, 'response')
        result = json.loads(text)
        self.assertEqual(len(result['items']), 120)
        self.assertEqual(result['last'], 'FINAL_CONTENT')
        self.assertNotIn('PRIVATE', text)
        self.assertNotIn('FINAL_CONTENT', json.dumps(self.app.snapshot('all')))
        self.assertFalse(initial['truncated'])
        old = initial['response']
        self.write('response_item', {'type': 'custom_tool_call_output', 'call_id': 'large', 'output': 'changed'})
        status, _ = self.get('/api/codex/tool?'+urlencode(query | {'lazy': '1', 'field': 'response', 'offset': old['next'], 'revision': old['revision']}))
        self.assertEqual(status, 409)

    def test_sql_mask_on_off_and_invalid_page_queries(self):
        sql = "UPDATE items SET password='PRIVATE', note='"+'x'*70000+"';"
        self.write('response_item', {'type': 'function_call', 'name': 'mcp__sqlite__query', 'call_id': 'sql', 'arguments': json.dumps({'database': 'demo.db', 'query': sql})})
        self.app.refresh()
        event = self.app.cache['all']['codex']['sqlite']['events'][0]
        query = {'id': event['id']}
        masked, _ = self.pages('/api/codex/sql', query, 'sql')
        self.assertNotIn('PRIVATE', masked)
        original, _ = self.pages('/api/codex/sql', query | {'mask': '0'}, 'sql')
        self.assertEqual(original, sql.rstrip(';'))
        for extra in ('&mask=0&mask=1', '&lazy=1&lazy=1', '&lazy=0', '&lazy=1&offset=0', '&lazy=1&path=private'):
            self.assertEqual(self.get('/api/codex/sql?'+urlencode(query)+extra)[0], 400)
        self.app.observations['sqlite'] = False
        self.assertEqual(self.get('/api/codex/sql?'+urlencode(query | {'lazy': '1'}))[0], 409)

    def test_diagnostic_error_body_on_demand_and_changed_desktop_line_rejected(self):
        stamp = datetime.now(timezone.utc).timestamp()
        path = self.home/'logs_10.sqlite'
        with closing(sqlite3.connect(path)) as db, db:
            db.execute('CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,feedback_log_body TEXT)')
            db.execute('INSERT INTO logs VALUES(1,?,\'ERROR\',\'codex_api::endpoint\',?)', (stamp, 'websocket failure '+('detail '*10000)+' password="PRIVATE" FINAL_ERROR'))
        self.app.refresh()
        event = next(event for event in self.app.diagnostics.events if event['source']=='codex_core')
        text, _ = self.pages('/api/codex/error', {'id': event['content_id']}, 'text')
        self.assertIn('FINAL_ERROR', text)
        self.assertNotIn('PRIVATE', text)
        self.assertNotIn('FINAL_ERROR', json.dumps(self.app.snapshot('all')))
        checkpoint=self.home/'monitoring/local-activity-monitor-errors.json'
        if checkpoint.exists():
            self.assertNotIn('FINAL_ERROR', checkpoint.read_text(encoding='utf-8'))
        desktop = self.home/'codex-desktop-fixture.log'
        line = datetime.now(timezone.utc).isoformat()+' error [git] could not read repository password="PRIVATE"'
        desktop.write_bytes((line+'\n').encode('utf-8'))
        stat=desktop.stat()
        self.app.diagnostics.files[desktop]={'file_id': (stat.st_dev, stat.st_ino)}
        self.app.diagnostics.desktop_line(line, desktop.name, offset=0)
        identity=self.app.diagnostics.events[-1]['content_id']
        status, result=self.get('/api/codex/error?'+urlencode({'id': identity, 'lazy': '1'}))
        self.assertEqual(status, 200)
        self.assertIn('could not read repository', result['text']['text'])
        self.assertNotIn('PRIVATE', result['text']['text'])
        desktop.write_text(line.replace('could not', 'can now')+'\n', encoding='utf-8')
        self.assertIsNone(self.app.error_detail(identity)['text'])
        for query in ('id=../../private', 'id='+'f'*64+'&file=private'):
            self.assertEqual(self.get('/api/codex/error?'+query)[0], 400)

    def test_parse_over_one_mib_and_page_boundary_credentials(self):
        raw=json.dumps({'value': 'x'*1100000, 'last': 0, 'flags': [False, None], 'api_key': 'PRIVATE'})
        self.assertIsNotNone(parse_text(raw))
        value=complete_payload(raw)['value']
        self.assertEqual(len(value['value']), 1100000)
        self.assertEqual(value['flags'], [False, None])
        self.assertNotIn('PRIVATE', json.dumps(value))
        masked=complete_payload('x'*32760+' password="PRIVATE" FINAL')['value']
        first=content_page(masked)
        second=content_page(masked, first['next'], first['revision'])
        self.assertEqual(first['text']+second['text'], masked)
        self.assertNotIn('PRIVATE', first['text']+second['text'])
        with self.assertRaises(ValueError):
            content_page(masked, 0, 'f'*64)

    def test_jev_pages_preserve_large_result_and_require_observed_call(self):
        self.write('response_item', {'type': 'function_call', 'name': 'mcp__jev__jev_rank', 'call_id': 'jev-large', 'arguments': json.dumps({'query': 'observed'})})
        self.write('response_item', {'type': 'function_call_output', 'call_id': 'jev-large', 'output': json.dumps({'text': 'line\n'*20000, 'last': False, 'secret': 'PRIVATE'})})
        self.app.refresh()
        query = {'thread': THREAD, 'call': 'jev-large', 'index': '0'}
        text, _ = self.pages('/api/codex/jev', query, 'response')
        result = json.loads(text)
        self.assertEqual(len(result['text']), 100000)
        self.assertIs(result['last'], False)
        self.assertNotIn('PRIVATE', text)
        self.assertNotIn('line\\nline', json.dumps(self.app.snapshot('all')))
        self.assertEqual(self.get('/api/codex/jev?'+urlencode(query | {'call': 'unknown', 'lazy': '1'}))[0], 409)
        self.assertEqual(self.get('/api/codex/jev?'+urlencode(query | {'path': 'outside', 'lazy': '1'}))[0], 400)
        self.app.set_settings({'observations': {'jev_calls': False}})
        self.assertEqual(self.get('/api/codex/jev?'+urlencode(query | {'lazy': '1'}))[0], 409)
