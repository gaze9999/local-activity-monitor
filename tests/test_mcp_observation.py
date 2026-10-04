import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from unittest.mock import MagicMock

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.mcp_records import category, connection_state, discover_sources, response_metadata
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

    def test_tags_for_new_source_are_validated_copied_and_restored(self):
        self.call('mcp__future_service__read', {}, {'credits':5})
        dashboard=Dashboard(self.home,codex=True)
        dashboard.set_settings({'mcp_tags':{'future_service':[' Search ','Search','Docs']}})
        snapshot=dashboard.snapshot('24h')
        source=next(item for item in snapshot['mcp']['servers'] if item['server']=='future_service')
        self.assertEqual(source['tags'],['Search','Docs'])
        snapshot['settings']['mcp_tags']['future_service'].append('changed')
        self.assertEqual(dashboard.settings()['mcp_tags']['future_service'],['Search','Docs'])
        for value in (True,'tag',[''],['x'*41],['a']*5,['line\nbreak']):
            with self.assertRaises(ValueError):dashboard.set_settings({'interval':1,'mcp_tags':{'future_service':value}})
        self.assertEqual(dashboard.interval,10)
        dashboard.set_settings({'mcp_tags':{'future_service':[]}})
        self.assertNotIn('future_service',dashboard.settings()['mcp_tags'])
        self.assertEqual(category('unfamiliar','browser_screenshot'),'runtime')
        self.assertEqual(category('unfamiliar','extract_document'),'documents')

    def test_connection_observation_uses_local_evidence_only(self):
        event={'nested':False,'timestamp':'2026-10-04T00:00:00Z','completed_at':'2026-10-04T00:00:01Z'}
        self.assertEqual(connection_state([])['state'],'unconfirmed')
        self.assertEqual(connection_state([event|{'nested':True}])['state'],'unconfirmed')
        self.assertEqual(connection_state([event])['state'],'response')
        self.assertEqual(connection_state([event|{'completed_at':None}])['state'],'pending')
        self.assertEqual(connection_state([event],False)['state'],'paused')
        failure={'timestamp':'2026-10-04T00:00:02Z','code':'connection_closed'}
        self.assertEqual(connection_state([event],diagnostics=[failure])['state'],'error')
        self.assertEqual(connection_state([event],diagnostics=[failure|{'code':'error'}])['state'],'response')
        concurrent=connection_state([event,event|{'completed_at':None}])
        self.assertEqual(concurrent['state'],'response')
        self.assertEqual(concurrent['pending_calls'],1)

    def test_self_describing_metrics_and_unknown_tool_savings(self):
        result=response_metadata('new_source',{'metrics':{'latency':{'value':12.5,'unit':'ms'},'secret':{'value':99,'unit':'tokens'}},'saved_tokens':200,'reduction_percent':37.5,'recording_enabled':True,'raw_html':'PRIVATE'})
        self.assertEqual(result['metrics_latency_ms'],12.5)
        self.assertEqual(result['saved_tokens'],200)
        self.assertEqual(result['reduction_percent'],37.5)
        self.assertTrue(result['recording_enabled'])
        self.assertNotIn('secret',json.dumps(result));self.assertNotIn('PRIVATE',json.dumps(result))

    def test_source_recording_choice_is_only_read(self):
        from local_activity_monitor.collectors import monitor_config
        self.home.joinpath('config.toml').write_text('[mcp_servers.jev]\ncommand="fixture"\n',encoding='utf-8')
        Dashboard(self.home)
        self.assertFalse((self.home/'monitoring/jev-monitor.json').exists())
        with patch('local_activity_monitor.server.data_root',return_value=self.home/'data'):
            from local_activity_monitor.server import configure
            configure(self.home,True)
            Dashboard(self.home)
            self.assertTrue(monitor_config(self.home)[0])
            configure(self.home,False)
            Dashboard(self.home)
            self.assertFalse(monitor_config(self.home)[0])
        path=self.home/'monitoring/jev-monitor.json';path.write_text('{invalid',encoding='utf-8')
        Dashboard(self.home)
        self.assertEqual(path.read_text(),'{invalid')

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

    def test_unknown_mcp_metrics_are_bounded_and_exclude_private_fields(self):
        result = response_metadata('future', {'status':'ok', 'recording_enabled':False,
            'metrics': {'processed_count':7, 'latency_ms':12.5, 'private_text':'SECRET', 'api_key_count':99,
                        'access_token_bytes':24, 'user_id_count':456, 'negative_count':-1, 'invalid_ms':float('inf')},
            'private_number':123, 'text':'PRIVATE', 'raw_sql':'PRIVATE'})
        self.assertEqual(result['metrics_processed_count'],7)
        self.assertEqual(result['metrics_latency_ms'],12.5)
        self.assertFalse(result['recording_enabled'])
        self.assertEqual(set(result),{'status','recording_enabled','metrics_processed_count','metrics_latency_ms'})
        self.assertNotIn('SECRET',json.dumps(result))
        capped = response_metadata('future', {'metrics':{f'metric{i}_count':i for i in range(100)}})
        self.assertLessEqual(len(capped),40)

    def test_recording_status_is_read_from_supported_sources(self):
        self.home.joinpath('config.toml').write_text('[mcp_servers.future]\ncommand="PRIVATE"\n',encoding='utf-8')
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh()
        self.assertEqual(dashboard.snapshot('24h')['mcp']['recording_status'],{})
        self.home.joinpath('config.toml').write_text('[mcp_servers.jev]\ncommand="PRIVATE"\n',encoding='utf-8')
        dashboard.refresh()
        self.assertEqual(dashboard.snapshot('24h')['mcp']['recording_status']['jev']['enabled'],False)
        self.assertNotIn('PRIVATE',json.dumps(dashboard.snapshot('24h')['mcp']))

    def test_mcp_usage_and_credit_snapshots_keep_units_and_exclude_private_fields(self):
        result=response_metadata('future', {'usage':{'input_tokens':120, 'output_tokens':40,
            'credits':{'balance':'18.125', 'remaining':0, 'used':False, 'limit':float('nan'),
                'has_credits':False, 'unlimited':True, 'account_id':'PRIVATE', 'note':'SECRET'}},
            'cost_credits':2.5, 'credits_used':3, 'secret_credits':9})
        self.assertEqual(result['usage_input_tokens'],120)
        self.assertEqual(result['usage_output_tokens'],40)
        self.assertEqual(result['usage_credits_balance'],18.125)
        self.assertEqual(result['usage_credits_remaining'],0)
        self.assertFalse(result['usage_credits_has_credits'])
        self.assertTrue(result['usage_credits_unlimited'])
        self.assertEqual(result['cost_credits'],2.5)
        self.assertEqual(result['credits_used'],3)
        for key in ('usage_credits_used','usage_credits_limit','secret_credits'):
            self.assertNotIn(key,result)
        self.assertNotIn('PRIVATE',json.dumps(result));self.assertNotIn('SECRET',json.dumps(result))
        capped=response_metadata('future', {'metrics':{f'metric{i}_count':i for i in range(100)},
            'credits':{'balance':1,'remaining':0,'unlimited':False}})
        self.assertLessEqual(len(capped),40)

    def test_source_switch_and_reenable_reparse(self):
        self.call('mcp__future__read', {}, {'status':'ok'})
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh()
        dashboard.set_settings({'mcp_sources':{'future':False}})
        self.assertEqual(dashboard.snapshot('24h')['mcp']['events'],[])
        dashboard.set_settings({'mcp_sources':{'future':True}})
        self.assertEqual(dashboard.snapshot('24h')['mcp']['events'][0]['result']['status'],'ok')

    def test_mcp_description_from_local_readme_and_custom_override(self):
        directory=self.home/'future';directory.mkdir();directory.joinpath('server.py').write_text('PRIVATE_SOURCE',encoding='utf-8')
        directory.joinpath('README.md').write_text('# Future\n\nProvides local document indexes\n\n```python\nPRIVATE_SOURCE\n```',encoding='utf-8')
        self.home.joinpath('config.toml').write_text('[mcp_servers.future]\ncommand="python"\nargs=['+json.dumps(str(directory/'server.py'))+']\n[mcp_servers.future.env]\nKEY="SECRET"\n',encoding='utf-8')
        dashboard=Dashboard(self.home,codex=True);dashboard.refresh()
        source=dashboard.snapshot('24h')['mcp']['servers'][0]
        self.assertEqual(source['description'],'Provides local document indexes')
        self.assertEqual(source['description_source'],'README.md')
        self.assertNotIn('SECRET',json.dumps(source));self.assertNotIn('PRIVATE_SOURCE',json.dumps(source))
        dashboard.set_settings({'mcp_descriptions':{'future':'Custom purpose'}})
        self.assertEqual(dashboard.snapshot('24h')['mcp']['servers'][0]['description'],'Custom purpose')
        dashboard.set_settings({'mcp_descriptions':{'future':''}})
        self.assertEqual(dashboard.snapshot('24h')['mcp']['servers'][0]['description_source'],'README.md')
        with self.assertRaises(ValueError):dashboard.set_settings({'mcp_descriptions':{'../escape':'x'}})

    def test_config_description_is_supported_without_tomllib(self):
        self.home.joinpath('config.toml').write_text('[mcp_servers.future]\ndescription="Indexes local files"\ncommand="PRIVATE"\n',encoding='utf-8')
        for parser in (None, __import__('local_activity_monitor.mcp_records',fromlist=['tomllib']).tomllib):
            details={}
            with patch('local_activity_monitor.mcp_records.tomllib',parser):
                self.assertEqual(discover_sources(self.home,details),{'future':'configured'})
            self.assertEqual(details['future']['description_source'],'config')
            self.assertEqual(details['future']['description'],'Indexes local files')

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
