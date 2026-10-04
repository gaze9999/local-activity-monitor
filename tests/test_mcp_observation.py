import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from unittest.mock import MagicMock

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.mcp_records import discover_sources, response_metadata
from local_activity_monitor.server import Dashboard, handler

THREAD = '00000000-0000-0000-0000-000000000001'


class McpObservationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
        self.root = self.home/'sessions'
        self.root.mkdir()
        self.path = self.root/f'rollout-{THREAD}.jsonl'

    def tearDown(self):
        self.temp.cleanup()

    def write(self, kind, payload, when='2026-10-03T00:00:00Z'):
        with self.path.open('a', encoding='utf-8') as stream:
            stream.write(json.dumps({'type': kind, 'payload': payload, 'timestamp': when})+'\n')

    def call(self, name, args, output, call='c'):
        self.write('response_item', {'type':'function_call','name':name,'arguments':json.dumps(args),'call_id':call})
        self.write('response_item', {'type':'function_call_output','output':json.dumps(output),'call_id':call}, '2026-10-03T00:00:02Z')

    def test_source_discovery_tracks_unknown_sources_and_skips_disabled_and_secrets(self):
        self.home.joinpath('config.toml').write_text('[mcp_servers.future_service]\ncommand="PRIVATE"\n[mcp_servers.future_service.env]\nKEY="SECRET"\n[mcp_servers.disabled]\nenabled=false\n[plugins."code-review@openai-bundled"]\nenabled=true\n', encoding='utf-8')
        for compatibility in (False, True):
            with patch('local_activity_monitor.mcp_records.tomllib', None) if compatibility else patch('local_activity_monitor.mcp_records.PLUGIN_SOURCES', {'code-review':'code_review'}):
                self.assertEqual(discover_sources(self.home), {'future_service':'configured','code_review':'configured'})

    def test_document_result_projection_and_mixed_exec_isolation(self):
        self.call('mcp__local_documents__extract_document', {'source':'/private/report.pdf','ocr':'auto','query':'PRIVATE'}, {'content':[{'type':'text','text':json.dumps({'status':'partial','content':'SECRET','content_chars':123,'ocr':{'processed_items':2,'omitted_items':3,'errors':[{'reason':'PRIVATE'}]}})}]})
        self.write('response_item', {'type':'custom_tool_call','name':'exec','call_id':'mixed','input':'await tools.mcp__local_documents__extract_document({source:"/private/a.pdf"}); await tools.exec_command({cmd:"cat SECRET"});'})
        self.write('response_item', {'type':'custom_tool_call_output','call_id':'mixed','output':json.dumps({'status':'failed','content':'PRIVATE'})})
        collector=CodexCollector(self.root);collector.refresh();events=collector.snapshot()['mcp_events']
        direct=next(e for e in events if not e['nested']);nested=next(e for e in events if e['nested'])
        self.assertEqual(direct['metadata']['format'],'pdf')
        self.assertEqual(direct['result']['ocr_processed_items'],2)
        self.assertEqual(direct['duration_ms'],2000)
        self.assertEqual(nested['result'],{})
        self.assertIsNone(nested['duration_ms'])
        self.assertNotIn('SECRET',json.dumps(events));self.assertNotIn('PRIVATE',json.dumps(events))

    def test_unknown_tool_shape_does_not_hide_calls_or_break_other_sources(self):
        self.call('mcp__future__new_tool', {'private':'SECRET'}, ['future output shape'])
        self.call('mcp__workspace_inspection__validation_evidence', {}, {'runs':[None,{'results':None},{'results':[{'status':'passed'},{'status':'failed'}]}]}, 'evidence')
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh();data=dashboard.snapshot('24h')
        self.assertEqual({s['server'] for s in data['mcp']['servers']},{'future','workspace_inspection'})
        future=next(e for e in data['mcp']['events'] if e['server']=='future')
        self.assertEqual(future['result'],{})
        self.assertEqual(future['category'],'other')
        dashboard.set_settings({'mcp_categories':{'future':'apps'}})
        self.assertEqual(next(e for e in dashboard.snapshot('24h')['mcp']['events'] if e['server']=='future')['category'],'apps')
        self.assertFalse(data['availability']['jev'])
        self.assertNotIn('SECRET',json.dumps(data))

    def test_source_switch_and_reenable_reparse(self):
        self.call('mcp__future__read', {}, {'status':'ok'})
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh()
        dashboard.set_settings({'mcp_sources':{'future':False}})
        self.assertEqual(dashboard.snapshot('24h')['mcp']['events'],[])
        dashboard.set_settings({'mcp_sources':{'future':True}})
        self.assertEqual(dashboard.snapshot('24h')['mcp']['events'][0]['result']['status'],'ok')

    def test_namespace_keeps_identical_tool_names_on_separate_servers(self):
        for namespace, call in [('mcp__future_one','one'),('mcp__future_two','two')]:
            self.write('response_item', {'type':'function_call','name':'read','namespace':namespace,'arguments':'{}','call_id':call})
            self.write('response_item', {'type':'function_call_output','call_id':call,'output':'{"status":"ok"}'})
        self.write('response_item', {'type':'function_call','name':'run','namespace':'web','arguments':'{}','call_id':'web'})
        collector=CodexCollector(self.root);collector.refresh();data=collector.snapshot()
        self.assertEqual(set(data['tools']), {'mcp__future_one__read','mcp__future_two__read','web.run'})
        self.assertEqual({event['server'] for event in data['mcp_events']}, {'future_one','future_two','web'})

    def test_web_references_strip_query_credentials_and_internal_hosts(self):
        self.call('web__run', {'open':[{'ref_id':'https://example.org/page?token=SECRET#fragment'},{'ref_id':'http://127.0.0.1/private'}]}, {'url':'https://example.org/reference?api_key=SECRET','content':'PRIVATE'})
        collector=CodexCollector(self.root);collector.refresh();events=collector.snapshot()['mcp_events']
        self.assertEqual(events[0]['metadata']['references'],['https://example.org/page'])
        self.assertEqual(events[0]['result']['references'],['https://example.org/reference'])
        self.assertNotIn('SECRET',json.dumps(events));self.assertNotIn('PRIVATE',json.dumps(events))
        self.assertEqual(response_metadata('web', {'content':None})['references'],[])

    def test_patch_paths_and_task_duration_pairing(self):
        self.write('event_msg', {'type':'task_started'})
        self.write('response_item', {'type':'custom_tool_call','name':'exec','call_id':'patch','input':'await tools.apply_patch("*** Begin Patch\\n*** Update File: src/a.py\\n@@\\n+SECRET\\n*** End Patch");'})
        self.write('event_msg', {'type':'task_complete'},'2026-10-03T00:01:00Z')
        self.write('event_msg', {'type':'task_started'},'2026-10-03T00:02:00Z')
        self.write('event_msg', {'type':'task_complete'},'2026-10-03T00:02:30Z')
        collector=CodexCollector(self.root);collector.refresh();thread=collector.snapshot()['threads'][0]
        self.assertEqual(thread['task_duration_ms'],90000)
        self.assertEqual(thread['task_runs'],2)
        self.assertEqual(thread['file_changes'][0]['path'],'src/a.py')
        self.assertNotIn('SECRET',json.dumps(thread))

    def test_settings_are_validated_before_any_mutation(self):
        dashboard=Dashboard(self.home,codex=True)
        for value in ({'max_files':0},{'max_files':True},{'mcp_sources':{'future':1}},{'mcp_categories':{'future':'new'}},{'tool_descriptions':{'bad/name':'text'}},{'tool_descriptions':{'future':42}}):
            with self.assertRaises(ValueError):dashboard.set_settings(value)
        self.assertEqual(dashboard.max_files,20)
        dashboard.set_settings({'max_files':50,'tool_descriptions':{'mcp__future__new':'自訂用途'}})
        self.assertEqual(dashboard.settings()['tool_descriptions']['mcp__future__new'],'自訂用途')

    def test_skill_documents_only_load_observed_paths_and_keep_secrets_hidden(self):
        skill_dir=self.home/'skills'/'example'
        skill_dir.mkdir(parents=True)
        skill_dir.joinpath('SKILL.md').write_text('# Example\napi_key="SECRET"\n說明',encoding='utf-8')
        skill_dir.joinpath('README.md').write_text('# 使用方式\n可讀文件',encoding='utf-8')
        self.call('exec_command', {'cmd':f'Get-Content "{skill_dir}/SKILL.md"'}, {})
        collector=CodexCollector(self.root);collector.refresh()
        self.assertEqual(collector.snapshot()['skills']['events'][0]['skill'],'example')
        self.assertNotIn(str(self.home),json.dumps(collector.snapshot()['skills']))
        documents=collector.skill_detail('example')['documents']
        self.assertEqual({item['name'] for item in documents},{'SKILL.md','README.md'})
        self.assertNotIn('SECRET',json.dumps(documents))
        self.assertEqual(collector.skill_detail('not_observed')['documents'],[])
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh()
        dashboard.set_settings({'observations':{'skills':False}})
        self.assertIsNone(dashboard.skill_detail('example'))

    def test_skill_inventory_reads_nested_documents_but_never_python_source(self):
        directory=self.home/'skills'/'example';directory.mkdir(parents=True)
        directory.joinpath('SKILL.md').write_text('# Skill',encoding='utf-8')
        directory.joinpath('references').mkdir();directory.joinpath('scripts').mkdir()
        directory.joinpath('references/guide.md').write_text('# Guide\napi_key="SECRET"',encoding='utf-8')
        directory.joinpath('scripts/helper.py').write_text('PRIVATE_PYTHON_SOURCE',encoding='utf-8')
        directory.joinpath('data.bin').write_bytes(b'PRIVATE_BINARY')
        directory.joinpath('.env').write_text('PRIVATE_ENV',encoding='utf-8')
        directory.joinpath('auth.json').write_text('PRIVATE_AUTH',encoding='utf-8')
        self.call('exec_command',{'cmd':f'Get-Content "{directory}/SKILL.md"'}, {})
        collector=CodexCollector(self.root);collector.refresh()
        original=Path.open
        def guarded(path,*args,**kwargs):
            self.assertNotEqual(path.suffix,'.py')
            return original(path,*args,**kwargs)
        with patch.object(Path,'open',guarded):
            result=collector.skill_detail('example')
            inventory={item['relative_path']:item for item in result['files']}
            self.assertEqual(set(inventory),{'SKILL.md','references/guide.md','scripts/helper.py','data.bin'})
            self.assertEqual(inventory['scripts/helper.py']['kind'],'python')
            self.assertFalse(inventory['scripts/helper.py']['readable'])
            self.assertGreater(inventory['scripts/helper.py']['bytes'],0)
            guide=collector.skill_detail('example','references/guide.md')['documents']
            self.assertIn('# Guide',guide[0]['text']);self.assertNotIn('SECRET',json.dumps(guide))
            self.assertEqual(collector.skill_detail('example','scripts/helper.py')['documents'],[])
        self.assertNotIn('PRIVATE_',json.dumps(result))

    def test_skill_inventory_bounds_and_resolved_path_guard(self):
        directory=self.home/'skills'/'example';directory.mkdir(parents=True)
        directory.joinpath('SKILL.md').write_text('# Skill',encoding='utf-8')
        directory.joinpath('escape.md').write_text('PRIVATE',encoding='utf-8')
        outside=self.home/'outside.md';outside.write_text('OUTSIDE',encoding='utf-8')
        self.call('exec_command',{'cmd':f'Get-Content "{directory}/SKILL.md"'}, {})
        collector=CodexCollector(self.root);collector.refresh()
        original=Path.resolve
        def resolve(path,*args,**kwargs):
            return outside if path.name=='escape.md' else original(path,*args,**kwargs)
        with patch.object(Path,'resolve',resolve):
            result=collector.skill_detail('example')
            self.assertNotIn('escape.md',{item['relative_path'] for item in result['files']})
            self.assertEqual(collector.skill_detail('example','../outside.md')['documents'],[])
            self.assertEqual(collector.skill_detail('example',str(outside))['documents'],[])
        def loop(path,*args,**kwargs):
            if path.name=='escape.md':raise RuntimeError('symlink loop')
            return original(path,*args,**kwargs)
        with patch.object(Path,'resolve',loop):
            result=collector.skill_detail('example')
            self.assertNotIn('escape.md',{item['relative_path'] for item in result['files']})
            self.assertEqual(result['documents'][0]['name'],'SKILL.md')
        for index in range(205):directory.joinpath(f'file-{index}.md').write_text('document',encoding='utf-8')
        result=collector.skill_detail('example')
        self.assertLessEqual(len(result['files']),200);self.assertTrue(result['files_truncated'])
        self.assertIn('SKILL.md',{item['relative_path'] for item in result['files']})

    def test_skill_http_selection_validates_paths_and_observation_switch(self):
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh()
        instance=object.__new__(handler(dashboard,8787))
        instance.headers={'Host':'127.0.0.1:8787'};instance.reply=MagicMock()
        for query in ('skill=example&file=../secret.md','skill=example&file=/secret.md','skill=example&file=C:/secret.md','skill=example&file=refs%5Csecret.md','skill=example&file=','skill=example&file=a.md&file=b.md','skill=example&other=x','file=a.md'):
            with self.subTest(query=query):
                instance.path='/api/codex/skill?'+query;instance.do_GET()
                self.assertEqual(instance.reply.call_args.args[0],400)
        instance.path='/api/codex/skill?skill=example&file=references/guide.md';instance.do_GET()
        self.assertEqual(instance.reply.call_args.args[0],200)
        dashboard.set_settings({'observations':{'skills':False}});instance.do_GET()
        self.assertEqual(instance.reply.call_args.args[0],409)

    def test_skill_large_document_is_bounded(self):
        directory=self.home/'skills'/'example';directory.mkdir(parents=True)
        directory.joinpath('SKILL.md').write_text('# Skill',encoding='utf-8')
        directory.joinpath('guide.txt').write_text('A'*300000,encoding='utf-8')
        self.call('exec_command',{'cmd':f'Get-Content "{directory}/SKILL.md"'}, {})
        collector=CodexCollector(self.root);collector.refresh()
        document=collector.skill_detail('example','guide.txt')['documents'][0]
        self.assertTrue(document['truncated']);self.assertEqual(document['read_bytes'],262144)
        self.assertLessEqual(len(document['text']),32768)

    def test_file_reads_writes_and_nested_duration_keep_only_metadata(self):
        self.call('exec_command', {'cmd':'Get-Content -LiteralPath "src/a file.py" -TotalCount 10; Set-Content output.txt -Value "PRIVATE_CONTENT"', 'workdir':'D:/project'}, {})
        self.call('mcp__future_source__read_file', {'file_path':'docs/new.md','private':'PRIVATE_CONTENT'}, {}, call='future-file')
        collector=CodexCollector(self.root);collector.refresh();snapshot=collector.snapshot()
        events=snapshot['file_activity']['events']
        self.assertEqual({(e['path'],e['operation']) for e in events},{('src/a file.py','read'),('output.txt','write'),('docs/new.md','read')})
        self.assertEqual(snapshot['file_activity']['operations'],{'read':2,'write':1})
        self.assertEqual(len(snapshot['threads'][0]['file_reads']),2)
        self.assertEqual(len(snapshot['threads'][0]['file_changes']),1)
        self.assertNotIn('PRIVATE_CONTENT',json.dumps(snapshot));self.assertNotIn('cmd',json.dumps(events))
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh();dashboard.set_settings({'observations':{'files':False}})
        self.assertEqual(dashboard.snapshot('24h')['codex']['file_activity']['events'],[])

    def test_sed_expression_is_not_reported_as_a_file(self):
        from local_activity_monitor.operation_records import file_operations
        for command, operation in [("sed -n '1,20p' src/a.py", 'read'), ("sed -i 's/PRIVATE/NEW/' src/a.py", 'modified'), ("sed -i '' -e 's/PRIVATE/NEW/' src/a.py", 'modified'), ("sed --expression='1,20p' src/a.py", 'read')]:
            with self.subTest(command=command):
                events=file_operations([('exec_command', {'cmd':command}, False)])
                self.assertEqual(events,[{'path':'src/a.py','operation':operation,'tool':'exec_command','nested':False}])
        events=file_operations([('exec_command', {'cmd':'sed -f script.sed src/a.py'}, False)])
        self.assertEqual({(event['path'],event['operation']) for event in events},{('script.sed','read'),('src/a.py','read')})

    def test_file_parser_skips_dynamic_paths_and_retains_nested_identity(self):
        from local_activity_monitor.operation_records import file_operations
        calls=[('exec_command',{'cmd':'Get-Content "$path"; Get-Content "src/*.py"; cat docs/readme.md', 'workdir':'/project'},True)]
        self.assertEqual(file_operations(calls),[{'path':'docs/readme.md','operation':'read','tool':'exec_command','nested':True,'workdir':'/project'}])
        self.write('response_item',{'type':'custom_tool_call','name':'exec','call_id':'nested-file','input':'await tools.exec_command({cmd:"cat src/file.py"});'})
        self.write('response_item',{'type':'custom_tool_call_output','call_id':'nested-file','output':'PRIVATE_CONTENT'},'2026-10-03T00:00:02Z')
        collector=CodexCollector(self.root);collector.refresh();event=collector.snapshot()['file_activity']['events'][0]
        self.assertIsNone(event['duration_ms']);self.assertEqual(event['container_duration_ms'],2000)


if __name__ == '__main__':
    unittest.main()
