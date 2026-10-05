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
