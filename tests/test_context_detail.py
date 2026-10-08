"""Selected visible context and communication stay out of durable metadata."""
from contextlib import closing
from datetime import datetime, timezone
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock

from local_activity_monitor.payload_detail import mask_payloads
from local_activity_monitor.server import Dashboard, handler


class ContextDetailTests(unittest.TestCase):
    def test_selected_context_communication_masking_and_restart(self):
        with tempfile.TemporaryDirectory() as folder:
            home = Path(folder)
            sessions = home/'sessions'
            sessions.mkdir()
            identity = '00000000-0000-4000-8000-000000000001'
            stamp = datetime.now(timezone.utc).isoformat()
            payloads = [
                ('session_meta', {'id':identity}),
                ('response_item', {'type':'message','role':'user','content':[{'type':'input_text','text':'Visible context api_key=PRIVATE_KEY'}]}),
                ('response_item', {'type':'reasoning','content':[{'type':'reasoning_text','text':'HIDDEN_REASONING'}],'summary':[{'type':'summary_text','text':'Public summary'}]}),
                ('response_item', {'type':'message','role':'assistant','channel':'analysis','content':[{'type':'output_text','text':'HIDDEN_ANALYSIS'}]}),
                ('response_item', {'type':'message','role':'user','content':[{'type':'input_text','text':'Message Type: MESSAGE\nTask name: /root\nSender: /root/worker\nPayload:\nPRIVATE_MESSAGE'}]}),
                ('response_item', {'type':'function_call','name':'collaboration.send_message','call_id':'send1','arguments':json.dumps({'target':'/root/worker','message':'PRIVATE_BODY'})}),
                ('response_item', {'type':'function_call_output','call_id':'send1','output':'sent'}),
            ]
            path = sessions/('rollout-'+identity+'.jsonl')
            path.write_text(''.join(json.dumps({'timestamp':stamp,'type':kind,'payload':payload})+'\n' for kind,payload in payloads),encoding='utf-8')
            first = Dashboard(home,codex=True)
            first.codex.refresh()
            def get(query,headers=None):
                request = object.__new__(handler(first,8787))
                request.headers = headers or {'Host':'127.0.0.1:8787'}
                request.path = '/api/codex/context?thread_id='+identity+query
                request.reply = MagicMock()
                request.do_GET()
                return request.reply.call_args.args
            self.assertEqual(get('&mask=invalid')[0],400)
            self.assertEqual(get('&mask=0&mask=1')[0],400)
            self.assertEqual(get('&mask=0',{'Host':'attacker.invalid'})[0],403)
            self.assertNotIn('PRIVATE_KEY',get('')[1].decode())
            self.assertIn('PRIVATE_KEY',get('&mask=0')[1].decode())
            self.assertNotIn('PRIVATE_KEY',get('')[1].decode())
            result = first.codex.context_detail(identity,'send1')
            rendered = json.dumps(result)
            self.assertIn('Public summary',rendered)
            self.assertNotIn('HIDDEN_REASONING',rendered)
            self.assertNotIn('HIDDEN_ANALYSIS',rendered)
            self.assertNotIn('PRIVATE_KEY',rendered)
            with mask_payloads(False):
                self.assertIn('PRIVATE_KEY',json.dumps(first.codex.context_detail(identity)))
                self.assertIn('PRIVATE_BODY',json.dumps(first.codex.agent_message_detail(identity,'send1')))
            self.assertTrue(first.codex.context_detail(identity)['masked'])
            rows = first.codex.snapshot()['threads']
            self.assertEqual({event['direction'] for event in rows[0]['agent_messages']},{'incoming','outgoing'})
            self.assertNotIn('PRIVATE_MESSAGE',json.dumps(rows))
            with closing(first.codex.session_checkpoint.store.connect()) as db:
                metadata = ''.join(row[0] for row in db.execute('SELECT payload FROM session_cursors UNION ALL SELECT payload FROM session_calls'))
            self.assertNotIn('PRIVATE_BODY',metadata)
            self.assertNotIn('PRIVATE_MESSAGE',metadata)
            second = Dashboard(home,codex=True)
            second.codex.refresh()
            self.assertEqual(second.codex.read_bytes,0)
            self.assertEqual(len(second.codex.snapshot()['threads'][0]['agent_messages']),2)
