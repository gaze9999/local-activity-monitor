import io
import gzip
import base64
import hashlib
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import MagicMock

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.operation_records import git_commands, operations, redact, workflow_operations
from local_activity_monitor.server import Dashboard, handler


THREAD = "00000000-0000-0000-0000-000000000001"


class ActivityDetailsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
        self.root = self.home/"sessions"
        self.root.mkdir()
        self.path = self.root/("rollout-"+THREAD+".jsonl")

    def tearDown(self):
        self.temp.cleanup()

    def write(self, kind, payload, timestamp="2026-10-03T00:00:00Z"):
        with self.path.open("ab") as stream:
            stream.write(json.dumps({"type": kind, "payload": payload, "timestamp": timestamp}).encode()+b"\n")

    def request(self, dashboard, value, path="/api/jev/recording", headers=None):
        instance = object.__new__(handler(dashboard, 8787))
        raw = json.dumps(value).encode()
        instance.headers = {"Host": "127.0.0.1:8787", "Origin": "http://127.0.0.1:8787", "Content-Type": "application/json", "Content-Length": str(len(raw))} | (headers or {})
        instance.path, instance.rfile, instance.reply = path, io.BytesIO(raw), MagicMock()
        instance.do_POST()
        return instance.reply.call_args.args

    def test_live_title_and_classification_without_other_database_fields(self):
        self.write("session_meta", {"id": THREAD, "originator": "codex_work_desktop", "thread_source": "user"})
        with closing(sqlite3.connect(self.home/"state_5.sqlite")) as db, db:
            db.execute("CREATE TABLE threads(id TEXT,title TEXT,model TEXT,originator TEXT,thread_source TEXT,first_user_message TEXT)")
            db.execute("INSERT INTO threads VALUES(?,?,?,?,?,?)", (THREAD,"目前對話名稱","gpt-6.1-sol","codex_work_desktop","user","SECRET_PROMPT"))
        collector = CodexCollector(self.root)
        collector.refresh()
        row = collector.snapshot()["threads"][0]
        self.assertEqual(row["thread_name"], "目前對話名稱")
        self.assertEqual(row["activity_type"], "work")
        self.assertEqual(row["environment"], "local")
        self.assertEqual(row["created_at"], "2026-10-03T00:00:00.000Z")
        self.assertNotIn("SECRET", json.dumps(row))
        with closing(sqlite3.connect(self.home/"state_5.sqlite")) as db, db:
            db.execute("UPDATE threads SET title='重新命名'")
        self.assertEqual(collector.snapshot()["threads"][0]["thread_name"], "重新命名")

    def test_reasoning_and_execution_metadata_keep_only_selected_fields(self):
        self.write('session_meta',{'id':THREAD,'model_provider':'openai','cli_version':'0.160.0','context_window':256000,'parent_thread_id':THREAD,'git':{'branch':'feature/test','commit_hash':'a'*40,'repository_url':'PRIVATE_URL'},'base_instructions':'SECRET_PROMPT','creator_account_id':'PRIVATE_ACCOUNT'})
        self.write('turn_context',{'model':'gpt-6.1-sol','effort':'high','approval_policy':'on-request','sandbox_policy':{'type':'workspace-write','writable_roots':['PRIVATE_PATH']},'collaboration_mode':{'mode':'default','settings':{'developer_instructions':'SECRET'}}})
        self.write('event_msg',{'type':'token_count','info':{'model_context_window':272000}})
        collector=CodexCollector(self.root);collector.refresh();row=collector.snapshot()['threads'][0]
        self.assertEqual(row['reasoning_effort'],'high')
        self.assertEqual(row['execution']['context_window'],272000)
        self.assertEqual(row['execution']['git_branch'],'feature/test')
        self.assertEqual(row['execution']['git_commit'],'a'*40)
        self.assertEqual(row['execution']['sandbox_mode'],'workspace-write')
        self.assertEqual(row['execution']['collaboration_mode'],'default')
        self.assertEqual(row['execution']['model_provider'],'openai')
        self.assertNotIn('PRIVATE',json.dumps(row));self.assertNotIn('SECRET',json.dumps(row))
        self.write('turn_context',{'model':'future-model','reasoning_effort':'future-level'},'2026-10-03T00:00:05Z')
        collector.refresh();row=collector.snapshot()['threads'][0]
        self.assertEqual((row['model'],row['reasoning_effort']),('future-model','future-level'))
        self.write('turn_context',{'model':'other-model'},'2026-10-03T00:00:06Z')
        collector.refresh();self.assertIsNone(collector.snapshot()['threads'][0]['reasoning_effort'])

    def test_reasoning_merge_uses_latest_model_context_across_session_files(self):
        self.write('session_meta',{'id':THREAD})
        self.write('turn_context',{'model':'older-model','effort':'low'},'2026-10-03T00:00:02Z')
        self.write('event_msg',{'type':'task_complete'},'2026-10-03T00:00:10Z')
        other=self.root/('rollout-copy-'+THREAD+'.jsonl')
        other.write_text(json.dumps({'type':'turn_context','timestamp':'2026-10-03T00:00:05Z','payload':{'model':'newer-model','effort':'high'}})+'\n',encoding='utf-8')
        collector=CodexCollector(self.root);collector.refresh();row=collector.snapshot()['threads'][0]
        self.assertEqual((row['model'],row['reasoning_effort']),('newer-model','high'))
        self.assertEqual(row['context_updated_at'],'2026-10-03T00:00:05.000Z')

    def test_applied_thread_settings_update_reasoning_without_new_turn(self):
        self.write('session_meta',{'id':THREAD})
        self.write('turn_context',{'model':'old-model','effort':'low'})
        self.write('event_msg',{'type':'thread_settings_applied','thread_id':THREAD,'thread_settings':{'model':'future-model','reasoning_effort':'future-level','model_provider_id':'openai','service_tier':'priority','approvals_reviewer':'auto-review','cwd':'PRIVATE_PATH','personality':'PRIVATE_TEXT'}},'2026-10-03T00:00:05Z')
        self.write('turn_context',{'model':'out-of-order','effort':'high'},'2026-10-03T00:00:04Z')
        self.write('event_msg',{'type':'thread_settings_applied','thread_id':'other','thread_settings':{'model':'other-thread'}},'2026-10-03T00:00:06Z')
        self.write('event_msg',{'type':'thread_settings_applied','thread_settings':[]},'2026-10-03T00:00:07Z')
        collector=CodexCollector(self.root);collector.refresh();row=collector.snapshot()['threads'][0]
        self.assertEqual((row['model'],row['reasoning_effort']),('future-model','future-level'))
        self.assertEqual(row['execution']['model_provider'],'openai')
        self.assertEqual(row['execution']['service_tier'],'priority')
        self.assertNotIn('PRIVATE',json.dumps(row))

    def test_current_catalog_environment_survives_local_session_file(self):
        self.write('session_meta',{'id':THREAD})
        (self.home/'sqlite').mkdir()
        with closing(sqlite3.connect(self.home/'sqlite/codex-dev.db')) as db, db:
            db.execute('CREATE TABLE local_thread_catalog(host_id,thread_id,display_title,source_created_at,source_updated_at,source_kind,thread_source,project_id)')
            db.execute('INSERT INTO local_thread_catalog VALUES(?,?,?,?,?,?,?,?)',('remote-control:test',THREAD,'移轉工作',1790985600,1790985610,'cli','user',None))
        collector=CodexCollector(self.root);collector.refresh()
        self.assertEqual(collector.snapshot()['threads'][0]['environment'],'remote')

    def test_catalog_reasoning_and_execution_fallback_tolerates_old_schema(self):
        self.write('session_meta',{'id':THREAD})
        with closing(sqlite3.connect(self.home/'state_5.sqlite')) as db,db:
            db.execute('CREATE TABLE threads(id,title,model,reasoning_effort,model_provider,cli_version,approval_mode,sandbox_policy,git_branch,git_sha,first_user_message)')
            db.execute('INSERT INTO threads VALUES(?,?,?,?,?,?,?,?,?,?,?)',(THREAD,'設定驗證','gpt-6.1-sol','medium','openai','0.160.0','on-request',json.dumps({'type':'workspace-write','writable_roots':['PRIVATE']}),'main','b'*40,'SECRET'))
        collector=CodexCollector(self.root);collector.refresh();row=collector.snapshot()['threads'][0]
        self.assertEqual(row['reasoning_effort'],'medium')
        self.assertEqual(row['execution']['cli_version'],'0.160.0')
        self.assertEqual(row['execution']['sandbox_mode'],'workspace-write')
        self.assertNotIn('PRIVATE',json.dumps(row));self.assertNotIn('SECRET',json.dumps(row))

    def test_catalog_project_cloud_dot_and_schedule_metadata(self):
        (self.home/"sqlite").mkdir()
        with closing(sqlite3.connect(self.home/"sqlite/codex-dev.db")) as db, db:
            db.execute("CREATE TABLE local_thread_catalog(host_id,thread_id,display_title,source_created_at,source_updated_at,source_kind,thread_source,project_id)")
            db.execute("INSERT INTO local_thread_catalog VALUES(?,?,?,?,?,?,?,?)", ("chatgpt:account",THREAD,"雲端對話",1790985600,1790985610,"chatgpt",None,"p1"))
            db.execute("CREATE TABLE automations(id,kind,target_thread_id,status)")
            db.execute("CREATE TABLE automation_runs(thread_id,automation_id)")
            db.execute("INSERT INTO automations VALUES('a','cron',?,'ACTIVE')", (THREAD,))
            db.execute("INSERT INTO automation_runs VALUES(?,'a')", (THREAD,))
        (self.home/".codex-global-state.json").write_text(json.dumps({"local-projects":{"p1":{"name":"測試專案","rootPaths":["PRIVATE_PATH"]}},"electron-persisted-atom-state":{"orbit-outputs-v1":[{"threadId":THREAD,"artifact":"SECRET"}],"prompt-history":["SECRET"]}}),encoding="utf-8")
        collector = CodexCollector(self.root)
        collector.refresh()
        row = collector.snapshot()["threads"][0]
        self.assertEqual((row["activity_type"],row["environment"],row["project_name"],row["trigger"]), ("chat","cloud","測試專案","dot"))
        self.assertTrue(row["has_schedule"])
        self.assertIsNone(row["tool_calls"])
        self.assertEqual(row["tokens"], {})
        self.assertNotIn("SECRET",json.dumps(row));self.assertNotIn("PRIVATE_PATH",json.dumps(row))

    def test_index_fallback_uses_latest_name_and_tolerates_bad_lines(self):
        self.write("session_meta", {"id":THREAD})
        (self.home/"session_index.jsonl").write_text(json.dumps({"id":THREAD,"thread_name":"舊名稱"})+"\nbad\n"+json.dumps({"id":THREAD,"thread_name":"新名稱"})+"\n",encoding="utf-8")
        collector=CodexCollector(self.root);collector.refresh()
        self.assertEqual(collector.snapshot()["threads"][0]["thread_name"],"新名稱")

    def test_display_title_precedes_initial_session_title(self):
        self.write("session_meta",{"id":THREAD})
        (self.home/"sqlite").mkdir()
        with closing(sqlite3.connect(self.home/"sqlite/codex-dev.db")) as db,db:
            db.execute("CREATE TABLE local_thread_catalog(host_id,thread_id,display_title,source_created_at,source_updated_at,source_kind,thread_source,project_id)")
            db.execute("INSERT INTO local_thread_catalog VALUES(?,?,?,?,?,?,?,?)",("local",THREAD,"目前側邊欄名稱",1790985600,1790985610,"vscode","user",None))
        with closing(sqlite3.connect(self.home/"state_5.sqlite")) as db,db:
            db.execute("CREATE TABLE threads(id,title)")
            db.execute("INSERT INTO threads VALUES(?,?)",(THREAD,"PRIVATE_INITIAL_PROMPT"))
        collector=CodexCollector(self.root);collector.refresh();data=collector.snapshot()
        self.assertEqual(data["threads"][0]["thread_name"],"目前側邊欄名稱")
        self.assertNotIn("PRIVATE_INITIAL_PROMPT",json.dumps(data))

    def test_git_calls_deduplicate_and_link_completion_time(self):
        self.write("session_meta", {"id":THREAD})
        payload={"type":"custom_tool_call","name":"exec","call_id":"c1","input":'const r = await tools.exec_command({cmd:"git status --short; git -C D:\\\\repo diff",workdir:"D:\\\\repo"}); text(r.output);'}
        self.write("response_item",payload)
        self.write("response_item",payload)
        self.write("response_item",{"type":"custom_tool_call_output","call_id":"c1","output":"SECRET_COMMAND_OUTPUT"},"2026-10-03T00:00:02Z")
        collector=CodexCollector(self.root);collector.refresh()
        snapshot=collector.snapshot()
        self.assertEqual(snapshot["tools"],{"exec":1})
        self.assertEqual(snapshot["nested_tools"],{"exec_command":1})
        self.assertEqual(snapshot["threads"][0]["tool_events"][0]["nested_tools"],{"exec_command":1})
        self.assertEqual(snapshot["git"]["operations"],{"status":1,"diff":1})
        self.assertEqual(snapshot["git"]["events"][0]["duration_ms"],2000)
        self.assertEqual(snapshot["git"]["events"][0]["repository"],"repo")
        self.assertNotIn("SECRET",json.dumps(snapshot))
        self.assertIsNone(collector.jev_detail(THREAD,"c1",0)["response"])

    def test_literal_parser_does_not_execute_or_match_comments_and_strings(self):
        payload={"name":"exec","input":'// tools.exec_command({cmd:"git push"});\ntext("tools.exec_command({cmd: \'git clean\'})"); await tools.exec_command({cmd: "git status", workdir: "/repo"});'}
        git,_=operations(payload)
        self.assertEqual([item["operation"] for item in git],["status"])
        self.assertEqual(git_commands('echo "git push"; git --no-optional-locks -c safe.directory=/repo log; git -C "/repo path" diff'),["log","diff"])

    def test_jev_request_response_on_demand_and_credentials_redacted(self):
        self.write("session_meta",{"id":THREAD})
        self.write("response_item",{"type":"function_call","name":"mcp__jev__jev_rank","call_id":"jev1","arguments":json.dumps({"query":"核對候選內容","api_key":"SECRET","accessToken":"SECRET","candidates":[{"id":"a","text":"實際送出內容"}]})})
        self.write("response_item",{"type":"function_call_output","call_id":"jev1","output":json.dumps({"ranked":["a"],"authorization":"Bearer SECRET","input_tokens":12})})
        collector=CodexCollector(self.root);collector.refresh()
        snapshot=collector.snapshot()
        self.assertEqual(snapshot["threads"][0]["jev_calls"][0]["operation"],"rank")
        self.assertNotIn("實際送出內容",json.dumps(snapshot,ensure_ascii=False))
        result=collector.jev_detail(THREAD,"jev1",0)
        self.assertEqual(result["request"]["query"],"核對候選內容")
        self.assertEqual(result["response"]["ranked"],["a"])
        self.assertEqual(result["response"]["input_tokens"],12)
        self.assertNotIn("SECRET",json.dumps(result))

    def test_nested_jev_and_workflow_metadata(self):
        payload={"name":"exec","input":'await tools.mcp__jev__jev_evaluate({state: "實際內容", rubric: [{id: "a"}]}); await tools.exec_command({cmd: "Get-Content C:\\\\skills\\\\coding-prompt\\\\SKILL.md; python -m unittest discover", workdir: "C:\\\\repo"});'}
        _,calls=operations(payload)
        self.assertEqual(calls[0]["operation"],"evaluate")
        self.assertEqual(calls[0]["arguments"]["state"],"實際內容")
        self.assertTrue(calls[0]["nested"])
        skills,checks=workflow_operations(payload)
        self.assertEqual(skills,["coding-prompt"])
        self.assertEqual(checks[0]["operation"],"python -m unittest")
        skills,checks=workflow_operations({"name":"exec_command","arguments":json.dumps({"cmd":"echo 'npm test; cat /skills/not-used/SKILL.md'"})})
        self.assertEqual((skills,checks),([],[]))

    def test_nested_managed_python_validation_is_literal_only(self):
        command="& 'C:\\Runtime\\python.exe' -X utf8 -m unittest discover -s tests"
        source='await Promise.allSettled([tools.exec_command('+json.dumps({'cmd':command})+')]);'
        _,checks=workflow_operations({'name':'exec','input':source})
        self.assertEqual(len(checks),1)
        self.assertEqual(checks[0]['operation'],'python -m unittest')
        _,checks=workflow_operations({'name':'exec_command','arguments':json.dumps({'cmd':'Write-Output "python -m unittest"'})})
        self.assertEqual(checks,[])

    def test_nested_tool_names_include_dynamic_arguments_without_payloads(self):
        self.write("session_meta",{"id":THREAD})
        self.write("response_item",{"type":"custom_tool_call","name":"exec","call_id":"c","input":'text("tools.fake({})"); await tools.web__run({search_query:[{q:"PRIVATE_QUERY"}]}); await tools.exec_command(commandArgs); /* tools.fake({}) */'})
        collector=CodexCollector(self.root);collector.refresh();data=collector.snapshot()
        self.assertEqual(data["nested_tools"],{"web__run":1,"exec_command":1})
        self.assertNotIn("PRIVATE_QUERY",json.dumps(data))
        collector.features["tool_events"]=False
        self.assertEqual(collector.snapshot()["nested_tools"],{})

    def test_mixed_exec_response_is_not_exposed_as_jev_content(self):
        self.write("session_meta",{"id":THREAD})
        self.write("response_item",{"type":"custom_tool_call","name":"exec","call_id":"mixed","input":'await tools.mcp__jev__jev_rank({query:"查詢", candidates:[]}); await tools.exec_command({cmd:"cat private-file"});'})
        self.write("response_item",{"type":"custom_tool_call_output","call_id":"mixed","output":"PRIVATE_UNRELATED_CONTENT"})
        collector=CodexCollector(self.root);collector.refresh()
        result=collector.jev_detail(THREAD,"mixed",0)
        self.assertEqual(result["request"]["query"],"查詢")
        self.assertIsNone(result["response"])

    def test_jev_credentials_in_embedded_json_and_error_text_are_redacted(self):
        value={"content":[{"text":json.dumps({"accessToken":"PRIVATE_ACCESS","headers":{"X-API-Key":"PRIVATE_HEADER"},"input_tokens":12})},{"text":'Failed: "api_key": "PRIVATE KEY WITH SPACES"'}]}
        result=redact(value)
        self.assertNotIn("PRIVATE",json.dumps(result))
        self.assertEqual(json.loads(result["content"][0]["text"])["input_tokens"],12)

    def test_settings_validate_before_mutation_and_tracking_all(self):
        dashboard=Dashboard(self.home,codex=True,max_files=1)
        self.write("session_meta",{"id":THREAD})
        second=self.root/"rollout-00000000-0000-0000-0000-000000000002.jsonl"
        second.write_text(json.dumps({"type":"session_meta","payload":{"id":"00000000-0000-0000-0000-000000000002"}})+"\n")
        dashboard.refresh();self.assertEqual(dashboard.snapshot("24h")["codex"]["files"],2)
        for value in ({"interval":0},{"interval":True},{"interval":2.5},{"observations":{"git":1}},{"observations":{"unknown":True}}):
            with self.assertRaises(ValueError):dashboard.set_settings(value)
        self.assertEqual(dashboard.interval,10)
        dashboard.set_settings({"track_all":True,"interval":5,"observations":{"git":False}})
        self.assertEqual(dashboard.snapshot("24h")["codex"]["files"],2)
        self.assertFalse(dashboard.codex.features["git"])
        dashboard.set_settings({"observations":{"codex":False}})
        self.assertEqual(dashboard.snapshot("24h")["codex"]["health"],"disabled")

    def test_recording_endpoint_and_config_failure_preserve_existing_file(self):
        dashboard=Dashboard(self.home)
        self.assertEqual(self.request(dashboard,{"enabled":True})[0],200)
        path=self.home/"monitoring/jev-monitor.json"
        before=json.loads(path.read_text())
        self.assertTrue(before["enabled"])
        self.assertEqual(self.request(dashboard,{"enabled":False})[0],200)
        self.assertEqual(json.loads(path.read_text())["database"],before["database"])
        path.write_text("invalid")
        self.assertEqual(self.request(dashboard,{"enabled":True})[0],409)
        self.assertEqual(path.read_text(),"invalid")
        path.write_text("[]")
        self.assertEqual(self.request(dashboard,{"enabled":True})[0],409)
        self.assertEqual(path.read_text(),"[]")

    def test_mutation_guards_and_payload_whitelist(self):
        dashboard=Dashboard(self.home)
        for headers in ({"Host":"attacker.invalid"},{"Origin":"https://attacker.invalid"},{"Sec-Fetch-Site":"cross-site"}):
            self.assertEqual(self.request(dashboard,{"enabled":True},headers=headers)[0],403)
        for value in ({"enabled":"true"},{"enabled":1},{"enabled":True,"database":"PRIVATE"},[]):
            self.assertEqual(self.request(dashboard,value)[0],400)
        self.assertEqual(self.request(dashboard,{"enabled":True},headers={"Content-Type":"text/plain"})[0],400)
        self.assertEqual(self.request(dashboard,{"enabled":True},headers={"Content-Length":"9999"})[0],400)
        self.assertEqual(self.request(dashboard,{"enabled":True},headers={"Transfer-Encoding":"chunked"})[0],400)
        self.assertEqual(self.request(dashboard,{"enabled":True},path="/api/arbitrary")[0],405)
        self.assertFalse((self.home/"monitoring/jev-monitor.json").exists())

    def test_http_compression_preserves_payload_and_declared_length(self):
        for encoding in ("gzip, deflate", "identity", "gzip;q=0"):
            instance=object.__new__(handler(Dashboard(self.home),8787))
            instance.headers={"Accept-Encoding":encoding}
            instance.send_response=MagicMock();instance.send_header=MagicMock();instance.end_headers=MagicMock();instance.wfile=io.BytesIO()
            payload=json.dumps({"name":"測試", "events":[{"tool":"exec"}]*1000},ensure_ascii=False).encode()
            instance.reply(200,payload,"application/json")
            actual=instance.wfile.getvalue();headers=dict(call.args for call in instance.send_header.call_args_list)
            self.assertEqual(int(headers["Content-Length"]),len(actual))
            self.assertEqual(gzip.decompress(actual) if headers.get("Content-Encoding")=="gzip" else actual,payload)
            self.assertEqual("Content-Encoding" in headers,encoding=="gzip, deflate")

    def test_error_keepalive_only_for_bodyless_get(self):
        for command,headers,close in [('GET',{},False),('GET',{'Content-Length':'1'},True),('GET',{'Transfer-Encoding':'chunked'},True),('POST',{},True)]:
            with self.subTest(command=command,headers=headers):
                instance=object.__new__(handler(Dashboard(self.home),8787))
                instance.command=command;instance.headers=headers;instance.close_connection=False
                instance.send_response=MagicMock();instance.send_header=MagicMock();instance.end_headers=MagicMock();instance.wfile=io.BytesIO()
                instance.reply(400,b'Invalid request')
                self.assertEqual(instance.close_connection,close)
                self.assertEqual(dict(call.args for call in instance.send_header.call_args_list).get('Connection')=='close',close)

    def test_page_embeds_current_script_with_matching_csp_hash(self):
        instance=object.__new__(handler(Dashboard(self.home),8787))
        instance.headers={"Host":"127.0.0.1:8787"};instance.path="/";instance.reply=MagicMock()
        instance.do_GET()
        content=instance.reply.call_args.args[1]
        script=content.split(b"<script>",1)[1].split(b"</script>",1)[0]
        self.assertNotIn(b"\r",script)
        expected=base64.b64encode(hashlib.sha256(script).digest()).decode("ascii")
        self.assertEqual(instance.reply.call_args.kwargs["script_hash"],expected)
        style=content.split(b"<style>",1)[1].split(b"</style>",1)[0]
        self.assertNotIn(b"\r",style)
        self.assertEqual(instance.reply.call_args.kwargs["style_hash"],base64.b64encode(hashlib.sha256(style).digest()).decode("ascii"))
        self.assertNotIn(b'href="/style.css"',content)
        self.assertNotIn(b'src="/boot.js"',content)
        self.assertIn(b"AbortController",script)

    def test_log_endpoint_and_locale_asset_keep_request_boundaries(self):
        self.write("session_meta", {"id": THREAD})
        dashboard = Dashboard(self.home, codex=True);dashboard.refresh()
        instance = object.__new__(handler(dashboard, 8787))
        for path, headers, code in [
            ("/api/logs", {"Host": "127.0.0.1:8787"}, 200),
            ("/api/logs?path=auth.json", {"Host": "127.0.0.1:8787"}, 400),
            ("/api/logs", {"Host": "external.example:8787"}, 403),
            ("/locales.json", {"Host": "127.0.0.1:8787"}, 200),
            ("/../error_records.py", {"Host": "127.0.0.1:8787"}, 404),
        ]:
            instance.headers, instance.path, instance.reply = headers, path, MagicMock()
            instance.do_GET()
            self.assertEqual(instance.reply.call_args.args[0], code)
            if path == "/api/logs" and code == 200:
                data = json.loads(instance.reply.call_args.args[1])
                source = next(row for row in data["sources"] if row["source"] == "session")
                self.assertEqual(source["file_count"], 1)
                self.assertLessEqual(len(data["entries"]), 2000)
            if path == "/locales.json":
                packs = json.loads(instance.reply.call_args.args[1])
                self.assertEqual(set(packs["en"]), set(packs["ja"]))
                self.assertEqual(packs["en"]["設定"], "Settings")

    def test_http_reuses_connection_and_closes_rejected_request(self):
        class Socket:
            def __init__(self, request):
                self.input, self.output = io.BytesIO(request), io.BytesIO()
            def makefile(self, *args):
                return self.input
            def sendall(self, value):
                self.output.write(value)
            def settimeout(self, value):
                self.timeout = value
        get=b"GET /api/instance HTTP/1.1\r\nHost: 127.0.0.1:8787\r\n\r\n"
        sock=Socket(get+get.replace(b"\r\n\r\n",b"\r\nConnection: close\r\n\r\n"))
        handler(Dashboard(self.home),8787)(sock,('127.0.0.1',1),None)
        self.assertEqual(sock.output.getvalue().count(b"HTTP/1.1 200"),2)
        self.assertEqual(sock.timeout,15)
        sock=Socket(b"POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:8787\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nBAD!!"+get)
        handler(Dashboard(self.home),8787)(sock,('127.0.0.1',1),None)
        self.assertIn(b"Connection: close",sock.output.getvalue())
        self.assertNotIn(b"HTTP/1.1 200",sock.output.getvalue())

    def test_disabled_features_stop_parsing_and_metadata_reads(self):
        self.write("session_meta",{"id":THREAD})
        self.write("response_item",{"type":"function_call","name":"exec_command","call_id":"c","arguments":json.dumps({"cmd":"git status; cat /skills/test/SKILL.md; npm test"})})
        collector=CodexCollector(self.root)
        collector.features={key:False for key in collector.features}
        collector.refresh();data=collector.snapshot()
        self.assertEqual(data["git"]["total"],0)
        self.assertEqual(data["skills"]["events"],[])
        self.assertEqual(data["checks"],[])
        self.assertEqual(data["threads"][0]["tool_events"],[])


if __name__=="__main__":
    unittest.main()
