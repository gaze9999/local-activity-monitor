import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import MagicMock
from urllib.error import HTTPError
from urllib.request import Request, build_opener, ProxyHandler
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler

from local_activity_monitor.collectors import CodexCollector, JevCollector
from local_activity_monitor.server import Dashboard, configure, handler

urlopen = build_opener(ProxyHandler({})).open


class MonitorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.home = Path(self.temp.name)
    def tearDown(self):
        self.temp.cleanup()
    def write(self, path, record):
        with path.open('ab') as file:
            file.write(json.dumps(record).encode()+b'\n')
    def record(self, kind, payload, time='2026-10-02T00:00:00Z'):
        return {'type': kind, 'payload': payload, 'timestamp': time}
    def test_incremental_snapshot_not_summed_and_content_omitted(self):
        path = self.home/'rollout-00000000-0000-0000-0000-000000000001.jsonl'
        self.write(path, self.record('session_meta', {'id':'00000000-0000-0000-0000-000000000001','cwd':'PRIVATE_PATH','base_instructions':'SECRET'}))
        self.write(path, self.record('turn_context', {'model':'gpt-6.1-sol','summary':'SECRET'}))
        self.write(path, self.record('event_msg', {'type':'token_count','info':{'total_token_usage':{'input_tokens':100,'output_tokens':5,'total_tokens':105}}}))
        self.write(path, self.record('token_usage_record', {'thread_token_usage':{'input_tokens':100,'output_tokens':5,'total_tokens':105}}))
        self.write(path, self.record('response_item', {'type':'custom_tool_call','name':'exec','call_id':'call1','input':'SECRET_COMMAND'}))
        self.write(path, self.record('event_msg', {'type':'item_completed','item':{'type':'custom_tool_call','name':'exec','call_id':'call1','raw_content':'SECRET'}}))
        collector=CodexCollector(self.home)
        collector.refresh(); before=collector.read_bytes
        collector.refresh(); self.assertEqual(collector.read_bytes,before)
        data=collector.snapshot()
        self.assertEqual(data['threads'][0]['tokens']['total_tokens'],105)
        self.assertEqual(data['tools'],{'exec':1})
        self.assertNotIn('SECRET',json.dumps(data)); self.assertNotIn('PRIVATE',json.dumps(data))
        self.write(path,self.record('token_usage_record',{'thread_token_usage':{'total_tokens':110}},'2026-10-02T00:00:01Z'))
        collector.refresh(); self.assertEqual(collector.snapshot()['threads'][0]['tokens']['total_tokens'],110)
    def test_partial_line_truncation_and_bad_json(self):
        path=self.home/'rollout-00000000-0000-0000-0000-000000000001.jsonl'
        self.write(path,self.record('turn_context',{'model':'gpt-6-sol'}))
        collector=CodexCollector(self.home); collector.refresh()
        record=json.dumps(self.record('response_item',{'type':'function_call','name':'test','call_id':'c'})).encode()
        with path.open('ab') as stream: stream.write(record[:30])
        collector.refresh(); self.assertEqual(collector.snapshot()['observed_tool_calls'],0)
        with path.open('ab') as stream: stream.write(record[30:]+b'\ninvalid\n')
        collector.refresh(); self.assertEqual(collector.snapshot()['observed_tool_calls'],1)
        self.assertEqual(collector.malformed,1)
        path.write_bytes(b''); collector.refresh(); self.assertEqual(collector.snapshot()['observed_tool_calls'],0)
    def test_bounded_incremental_source_read(self):
        path=self.home/'rollout-00000000-0000-0000-0000-000000000001.jsonl'
        for _ in range(300): self.write(path,self.record('response_item',{'type':'message','content':'s'*2000}))
        self.write(path,self.record('token_usage_record',{'thread_token_usage':{'total_tokens':9}}))
        collector=CodexCollector(self.home,tail_bytes=8192); collector.refresh()
        with path.open('rb') as stream:
            head_bytes=len(stream.readline(65536))
        self.assertGreaterEqual(collector.read_bytes,8192+head_bytes)
        self.assertLessEqual(collector.read_bytes,8*1024*1024)
        self.assertTrue(collector.snapshot()['threads'][0]['partial_history'])
        self.assertNotIn('total_tokens',collector.snapshot()['threads'][0]['tokens'])
        for _ in range(100):
            if collector.files[path]['offset']==path.stat().st_size:
                break
            before=collector.read_bytes
            collector.refresh()
            self.assertLessEqual(collector.read_bytes-before,collector.READ_LIMIT)
        self.assertFalse(collector.snapshot()['threads'][0]['partial_history'])
        self.assertEqual(collector.snapshot()['threads'][0]['tokens']['total_tokens'],9)
    def test_config_opt_in_idempotent_and_disable_preserves_history_path(self):
        db=self.home/'state/events.sqlite'
        self.assertEqual(JevCollector(self.home).snapshot()['health'],'disabled')
        self.assertEqual(configure(self.home,True,db),'enabled')
        self.assertEqual(configure(self.home,True,db),'unchanged')
        self.assertEqual(configure(self.home,False),'disabled')
        self.assertEqual(json.loads((self.home/'monitoring/jev-monitor.json').read_text())['database'],str(db))
        self.assertFalse(db.exists())
    def test_http_read_only_and_local_origin_guard(self):
        dashboard=Dashboard(self.home); dashboard.refresh()
        cls=handler(dashboard,8787)
        instance=object.__new__(cls)
        instance.headers={'Host':'127.0.0.1:8787'};instance.path='/api/snapshot';instance.reply=MagicMock()
        instance.do_GET()
        self.assertEqual(instance.reply.call_args.args[0],200)
        self.assertEqual(json.loads(instance.reply.call_args.args[1])['label'],'本機觀察統計')
        for headers,path,code in [({'Host':'attacker.invalid'},'/',403),({'Host':'127.0.0.1:8787','Origin':'https://attacker.invalid'},'/',403),({'Host':'127.0.0.1:8787'},'/../server.py',404),({'Host':'127.0.0.1:8787'},'/api/snapshot?window=bad',400)]:
            instance.headers=headers;instance.path=path
            instance.do_GET();self.assertEqual(instance.reply.call_args.args[0],code)
        instance.do_POST();self.assertEqual(instance.reply.call_args.args[0],405)

if __name__=='__main__': unittest.main()
