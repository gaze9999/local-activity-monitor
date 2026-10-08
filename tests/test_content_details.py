import io
import json
from datetime import datetime, timezone
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import MagicMock
from urllib.parse import urlencode

from local_activity_monitor.server import Dashboard, handler

THREAD='00000000-0000-0000-0000-000000000001'

class ContentDetailsTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home=Path(self.temp.name)
        if sys.platform=='darwin':
            self.home=self.home.resolve()
        self.root=self.home/'sessions'
        self.root.mkdir()
        self.path=self.root/('rollout-'+THREAD+'.jsonl')
        self.write('session_meta', {'id':THREAD})
        self.app=Dashboard(self.home,True,20)

    def write(self, kind, payload):
        with self.path.open('a',encoding='utf-8') as stream:
            stream.write(json.dumps({'type':kind,'timestamp':datetime.now(timezone.utc).isoformat(),'payload':payload})+'\n')

    def get(self, path):
        request=object.__new__(handler(self.app,8787))
        request.headers={'Host':'127.0.0.1:8787'}
        request.path=path
        request.reply=MagicMock()
        request.do_GET()
        args=request.reply.call_args.args
        return args[0],json.loads(args[1]) if args[0] in (200,409) else None

    def post(self, path, value, origin='http://127.0.0.1:8787'):
        request=object.__new__(handler(self.app,8787));raw=json.dumps(value).encode()
        request.path=path;request.rfile=io.BytesIO(raw);request.reply=MagicMock()
        request.headers={'Host':'127.0.0.1:8787','Origin':origin,'Content-Type':'application/json','Content-Length':str(len(raw))}
        request.do_POST();return request.reply.call_args.args[0]

    def test_exec_body_is_on_demand_and_masks_credentials(self):
        self.write('response_item',{'type':'custom_tool_call','name':'exec','call_id':'call_js','input':'const password="PRIVATE"; text(0);'})
        self.write('response_item',{'type':'custom_tool_call_output','call_id':'call_js','output':'{"count":0,"secret":"PRIVATE"}'})
        self.app.refresh()
        self.assertNotIn('text(0)',json.dumps(self.app.snapshot('24h')))
        result=self.app.tool_detail(THREAD,'call_js')
        self.assertIn('text(0)',result['request']);self.assertNotIn('PRIVATE',json.dumps(result))
        self.assertEqual(result['response']['count'],0)
        self.assertIsNone(self.app.tool_detail(THREAD,'unknown'))
        self.app.observations['tool_events']=False
        self.assertIsNone(self.app.tool_detail(THREAD,'call_js'))

    def test_apply_patch_content_and_call_identity(self):
        self.write('response_item',{'type':'custom_tool_call','name':'apply_patch','call_id':'call_patch','input':'*** Begin Patch\n*** Add File: example.txt\n+hello\n*** End Patch'})
        self.app.refresh()
        status,result=self.get('/api/codex/tool?'+urlencode({'thread_id':THREAD,'call_id':'call_patch'}))
        self.assertEqual(status,200);self.assertIn('+hello',result['request'])
        self.assertEqual(self.get('/api/codex/tool?thread_id='+THREAD+'&call_id=call_patch&path=private')[0],400)
        self.assertEqual(self.get('/api/codex/tool?thread_id='+THREAD+'&call_id=call_patch&call_id=other')[0],400)

    def test_file_size_summary_accepts_only_one_known_window(self):
        self.app.refresh()
        self.assertEqual(self.get('/api/codex/file-summary?window=all')[0], 200)
        for query in ('window=all&window=1h', 'window=invalid', 'window=all&path=private', 'window='):
            self.assertEqual(self.get('/api/codex/file-summary?'+query)[0], 400)

    def test_unknown_mcp_payload_and_disabled_source(self):
        self.write('response_item',{'type':'function_call','name':'mcp__future__evaluate','call_id':'call_mcp','arguments':json.dumps({'input':[0,False],'api_key':'PRIVATE'})})
        self.write('response_item',{'type':'function_call_output','call_id':'call_mcp','output':json.dumps({'answer':'ok','password':'PRIVATE'})})
        self.app.refresh();result=self.app.mcp_detail(THREAD,'call_mcp',0)
        self.assertEqual(result['request']['input'],[0,False]);self.assertNotIn('PRIVATE',json.dumps(result))
        self.assertEqual(result['response_scope'],'mcp_tool_call')
        self.assertIsNone(self.app.mcp_detail(THREAD,'call_mcp',1))
        self.app.mcp_sources['future']=False
        self.assertIsNone(self.app.mcp_detail(THREAD,'call_mcp',0))

    def test_mixed_exec_output_is_identified_as_outer_reply(self):
        self.write('response_item',{'type':'custom_tool_call','name':'exec','call_id':'call_mixed','input':'await tools.mcp__future__evaluate({input:0}); await tools.mcp__second__get({});'})
        self.write('response_item',{'type':'custom_tool_call_output','call_id':'call_mixed','output':'[{"first":1},{"second":2}]'})
        self.app.refresh();result=self.app.mcp_detail(THREAD,'call_mixed',0)
        self.assertEqual(result['response_scope'],'containing_tool_call')
        self.assertEqual(result['response'][1]['second'],2)
        self.assertNotIn('"first": 1',json.dumps(self.app.snapshot('24h')))

    def test_nested_dynamic_input_preserves_recorded_expression_without_evaluation(self):
        source = 'const result = await tools.mcp__textlint__lintFile({filePath: file, language:"zh-TW", password:"PRIVATE"}); text(result);'
        self.write('response_item', {'type':'custom_tool_call','name':'exec','call_id':'dynamic','input':source})
        self.write('response_item', {'type':'custom_tool_call_output','call_id':'dynamic','output':'{"messages":[],"ok":false}'})
        self.app.refresh()
        detail = self.app.mcp_detail(THREAD, 'dynamic', 0)
        self.assertEqual(detail['request_source'], 'recorded_expression')
        self.assertIn('filePath: file', detail['request'])
        self.assertIn('language:"zh-TW"', detail['request'])
        self.assertNotIn('PRIVATE', json.dumps(detail))
        self.assertEqual(detail['response']['ok'], False)
        self.assertEqual(detail['response_scope'], 'containing_tool_call')
        self.assertNotIn('filePath: file', json.dumps(self.app.snapshot('all')))

    def test_observed_positions_keep_old_details_after_tail_moves_and_reject_changed_record(self):
        self.app.codex.tail_bytes = 4096
        self.write('response_item', {'type':'function_call','name':'mcp__future__evaluate','call_id':'old','arguments':'{"input":0}'})
        self.write('response_item', {'type':'function_call_output','call_id':'old','output':'{"answer":false}'})
        self.app.refresh()
        for _ in range(10):
            self.write('response_item', {'type':'message','content':'PRIVATE_PROMPT'*200})
        self.app.refresh()
        detail = self.app.mcp_detail(THREAD, 'old', 0)
        self.assertEqual(detail['request'], {'input':0})
        self.assertEqual(detail['response'], {'answer':False})
        state = next(iter(self.app.codex.files.values()))
        with self.path.open('r+b') as stream:
            stream.seek(state['calls']['old']['request_offset'])
            stream.write(b'{}\n')
        self.assertIsNone(self.app.mcp_detail(THREAD, 'old', 0)['request'])

    def test_restart_rebuilds_only_retained_call_positions_with_bounded_incremental_reads(self):
        self.write('response_item', {'type':'function_call','name':'mcp__future__evaluate','call_id':'retained','arguments':'{"input":0}'})
        self.write('response_item', {'type':'function_call_output','call_id':'retained','output':'{"answer":false}'})
        self.write('response_item', {'type':'function_call','name':'exec_command','call_id':'sql-old','arguments':json.dumps({'cmd':'sqlite3 demo.db "SELECT 34"'})})
        self.write('response_item', {'type':'function_call_output','call_id':'sql-old','output':'{"exit_code":0}'})
        self.app.refresh()
        sql = next(event for event in self.app.cache['all']['codex']['sqlite']['events'] if event['statement']=='SELECT')
        for _ in range(100):
            self.write('response_item', {'type':'message','content':'PRIVATE_PROMPT'*1000})
        self.write('event_msg', {'type':'task_complete'})
        self.app.refresh()
        for _ in range(100):
            if self.app.codex.files[self.path]['offset']==self.path.stat().st_size:
                break
            self.app.refresh()
        self.assertEqual(self.app.codex.files[self.path]['offset'],self.path.stat().st_size)
        restarted = Dashboard(self.home,True,20)
        restarted.codex.tail_bytes = 4096
        restarted.codex.READ_LIMIT = 131072
        restarted.codex.features['errors'] = False
        restarted.refresh()
        self.assertEqual(restarted.codex.read_bytes,0)
        self.assertEqual(restarted.mcp_detail(THREAD,'retained',0)['content_status'],'available')
        for _ in range(20):
            before = restarted.codex.read_bytes
            restarted.refresh()
            self.assertLessEqual(restarted.codex.read_bytes-before,restarted.codex.READ_LIMIT)
            if restarted.mcp_detail(THREAD,'retained',0)['content_status']=='available':
                break
        detail = restarted.mcp_detail(THREAD,'retained',0)
        self.assertEqual(detail['request'],{'input':0})
        self.assertEqual(detail['response'],{'answer':False})
        self.assertEqual(restarted.sql_detail(sql['id'])['sql'],'SELECT 34')
        self.assertNotIn('PRIVATE_PROMPT',json.dumps(restarted.snapshot('all')))
        before = restarted.codex.read_bytes
        restarted.refresh()
        self.assertEqual(restarted.codex.read_bytes,before)

    def test_detail_backfill_keeps_progress_with_new_complete_and_pending_calls(self):
        for call in ('old-first', 'old-second'):
            self.write('response_item', {'type':'function_call','name':'mcp__future__evaluate','call_id':call,'arguments':'{"input":0}'})
            self.write('response_item', {'type':'function_call_output','call_id':call,'output':'{"answer":false}'})
        for _ in range(100):
            self.write('event_msg', {'type':'ignored','message':'fixture padding'*20})
        collector = self.app.codex
        collector.tail_bytes = 1024
        collector.features['errors'] = False
        collector.refresh()
        state = collector.files[self.path]
        # Exercise the retained-detail index after calls leave the working cache.
        state['calls'].pop('old-first',None)
        state['calls'].pop('old-second',None)
        self.assertNotIn('old-first', state['calls'])
        targets = [{'thread_id': THREAD, 'call_id': 'old-first'}]
        collector.select_detail_targets(targets)
        collector.related_budget = 512
        collector.backfill_detail_index()
        index = collector.detail_indexes[THREAD]
        cursor = index['cursor']
        self.assertGreater(cursor, 0)

        def recent(payload):
            offset = self.path.stat().st_size
            self.write('response_item', payload)
            with self.path.open('rb') as stream:
                stream.seek(offset)
                collector.parse(state, stream.readline(), offset)

        for number in range(4):
            call = 'recent-'+str(number)
            recent({'type':'function_call','name':'mcp__future__evaluate','call_id':call,'arguments':'{"input":0}'})
            recent({'type':'function_call_output','call_id':call,'output':'{"answer":false}'})
            targets.append({'thread_id': THREAD, 'call_id': call})
            collector.select_detail_targets(targets)
            collector.related_budget = 0
            before = collector.read_bytes
            collector.backfill_detail_index()
            self.assertEqual(index['cursor'], cursor)
            self.assertEqual(collector.read_bytes, before)
            self.assertIn('response_offset', index['positions'][call])
            collector.related_budget = 512
            collector.backfill_detail_index()
            self.assertLess(index['cursor'], cursor)
            self.assertLessEqual(collector.read_bytes-before, 512)
            cursor = index['cursor']

        recent({'type':'function_call','name':'mcp__future__evaluate','call_id':'pending','arguments':'{"input":0}'})
        targets.append({'thread_id': THREAD, 'call_id': 'pending'})
        collector.select_detail_targets(targets)
        collector.related_budget = 0
        collector.backfill_detail_index()
        self.assertEqual(index['cursor'], cursor)
        self.assertIn('request_offset', index['positions']['pending'])
        self.assertNotIn('response_offset', index['positions']['pending'])
        for _ in range(100):
            before = collector.read_bytes
            collector.related_budget = 1024
            collector.backfill_detail_index()
            self.assertLessEqual(collector.read_bytes-before, 1024)
            if index['cursor'] == 0:
                break
        self.assertEqual(index['cursor'], 0)
        self.assertIn('response_offset', index['positions']['old-first'])

        targets.append({'thread_id': THREAD, 'call_id': 'old-second'})
        collector.select_detail_targets(targets)
        collector.related_budget = 0
        collector.backfill_detail_index()
        self.assertEqual(index['cursor'], self.path.stat().st_size)
        for _ in range(100):
            collector.related_budget = 1024
            collector.backfill_detail_index()
            if 'response_offset' in index['positions'].get('old-second', {}):
                break
        self.assertIn('response_offset', index['positions']['old-second'])
        recent({'type':'function_call_output','call_id':'pending','output':'{"answer":true}'})
        collector.related_budget = 0
        collector.backfill_detail_index()
        self.assertIn('response_offset', index['positions']['pending'])

    def test_local_document_allowlist_save_conflict_and_origin(self):
        directory=self.home/'unknown';directory.mkdir();entry=directory/'server.mjs';entry.write_text('',encoding='utf-8')
        settings=directory/'settings.json';settings.write_text('{"enabled":true}',encoding='utf-8')
        self.home.joinpath('config.toml').write_text('[mcp_servers.future]\ncommand="node"\nargs=['+json.dumps(str(entry))+']\n',encoding='utf-8')
        files=self.app.mcp_documents('future')['files'];file=next(file for file in files if file['name']=='settings.json')
        selected=self.app.mcp_documents('future',file['id'])
        value={'server':'future','document':file['id'],'sha256':selected['sha256'],'text':'{"enabled":false}'}
        self.assertEqual(self.post('/api/mcp/file',value,origin='https://outside.example'),403)
        self.assertEqual(self.post('/api/mcp/file',value),200)
        self.assertEqual(json.loads(settings.read_text())['enabled'],False)
        self.assertEqual(self.post('/api/mcp/file',value),409)
        self.assertEqual(self.get('/api/mcp/files?server=future&document='+('a'*32))[0],409)
        self.assertEqual(self.get('/api/mcp/files?server=future&path=private')[0],400)

    def test_manual_resume_and_idle_settings_validation(self):
        self.assertEqual(self.app.settings()['idle_minutes'],5)
        for invalid in (True,-1,1441,'5'):
            with self.assertRaises(ValueError):self.app.set_settings({'idle_minutes':invalid})
        self.app.activity.paused=True
        self.assertEqual(self.post('/api/refresh',{}),200)
        self.assertFalse(self.app.activity.paused)
        self.assertEqual(self.post('/api/refresh',{'path':'private'}),400)
