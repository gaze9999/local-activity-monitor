from http.client import HTTPConnection
from http.server import ThreadingHTTPServer
from pathlib import Path
import tempfile
import json
import threading
import time
import unittest
from local_activity_monitor.server import Dashboard, handler


class EventStreamTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.dashboard=Dashboard(Path(self.temp.name),codex=False)
        self.server=ThreadingHTTPServer(('127.0.0.1',0),handler(self.dashboard,0))
        self.server.RequestHandlerClass=handler(self.dashboard,self.server.server_port)
        self.worker=threading.Thread(target=self.server.serve_forever,daemon=True)
        self.worker.start()
        self.addCleanup(self.close)

    def close(self):
        self.dashboard.stop.set()
        self.dashboard.notify_update()
        self.server.shutdown()
        self.server.server_close()
        self.worker.join(3)

    def connection(self):
        connection=HTTPConnection('127.0.0.1',self.server.server_port,timeout=3)
        self.addCleanup(connection.close)
        return connection

    def event(self, response):
        identity = response.readline()
        self.assertEqual(response.readline(), b'event: snapshot\n')
        raw = response.readline()
        self.assertTrue(raw.startswith(b'data: '))
        self.assertEqual(response.readline(), b'\n')
        value = json.loads(raw[6:])
        self.assertEqual(identity, f"id: {value['version']}\n".encode())
        return value

    def finish(self, response):
        try:
            return response.read()
        except ConnectionResetError:
            # Windows may reset a socket when the server explicitly ends SSE.
            return b''

    def test_initial_update_and_shutdown_release_stream_slot(self):
        connection=self.connection()
        connection.request('GET','/api/events')
        response=connection.getresponse()
        self.assertEqual(response.status,200)
        self.assertEqual(response.getheader('Content-Type'),'text/event-stream; charset=utf-8')
        value = self.event(response)
        self.assertEqual(value['version'], 0)
        self.assertEqual(value['window'], '24h')
        self.assertEqual(value['snapshot']['activity']['update_mode'], 'source_events')
        self.assertIn('codex', value['snapshot'])
        self.assertIn('entries', value['logs'])
        self.dashboard.notify_update()
        self.assertEqual(self.event(response)['version'], 1)
        self.dashboard.stop.set();self.dashboard.notify_update()
        self.assertEqual(self.finish(response),b'')
        self.assertTrue(self.dashboard.event_clients.acquire(False))
        self.dashboard.event_clients.release()

    def test_window_subscription_and_invalid_queries(self):
        connection=self.connection()
        connection.request('GET','/api/events?window=all')
        response=connection.getresponse()
        self.assertEqual(self.event(response)['window'], 'all')
        self.dashboard.stop.set();self.dashboard.notify_update()
        self.finish(response)
        self.dashboard.stop.clear()
        for query in ('window=bad','window=','window=24h&window=all','extra=1','window=all&extra=1'):
            connection=self.connection();connection.request('GET','/api/events?'+query)
            result=connection.getresponse()
            self.assertEqual(result.status,400)
            result.read()

    def test_debug_events_report_metrics_and_release_connection(self):
        self.dashboard.monitor.debug.set_enabled(True)
        self.dashboard.monitor.refreshed({'cpu_ms': 3, 'refresh_ms': 4, 'message': 'PRIVATE_PAYLOAD'})
        connection = self.connection()
        connection.request('GET', '/api/events')
        response = connection.getresponse()
        snapshot = self.event(response)
        self.assertTrue(snapshot['snapshot']['monitor']['debug']['enabled'])
        self.assertEqual(response.readline(), b'event: debug\n')
        line = response.readline()
        self.assertNotIn(b'PRIVATE_PAYLOAD', line)
        value = json.loads(line[6:])
        self.assertEqual(value['active_streams'], 1)
        self.assertEqual(value['records'][0]['kind'], 'stream_frame')
        self.assertGreater(value['records'][0]['transfer_bytes'], 0)
        self.assertEqual(value['records'][0]['snapshot_bytes'], len(json.dumps(snapshot, ensure_ascii=False, separators=(',', ':')).encode('utf-8')))
        self.assertGreater(value['records'][0]['transfer_bytes'], value['records'][0]['snapshot_bytes'])
        self.assertEqual(response.readline(), b'\n')
        self.dashboard.stop.set();self.dashboard.notify_update()
        response.close();connection.close()
        deadline = time.monotonic()+3
        while self.dashboard.monitor.debug.active_streams and time.monotonic() < deadline:
            threading.Event().wait(.01)
        self.assertEqual(self.dashboard.monitor.debug.active_streams, 0)

    def test_client_limit_and_local_guards(self):
        for _ in range(8):self.assertTrue(self.dashboard.event_clients.acquire(False))
        try:
            for path,headers,status in (('/api/events',{},503),)*8+(('/api/events?extra=1',{},400),('/api/events',{'Host':'evil.invalid'},403)):
                with self.subTest(path=path,headers=headers):
                    connection=self.connection();connection.request('GET',path,headers=headers)
                    response=connection.getresponse();self.assertEqual(response.status,status)
                    self.assertNotEqual(response.getheader('Connection'),'close')
                    self.assertEqual(response.read(),b'Local access only' if status==403 else b'Event stream unavailable')
                    connection.close()
        finally:
            for _ in range(8):self.dashboard.event_clients.release()

    def test_peer_close_releases_slots_during_rapid_window_switches(self):
        for index in range(20):
            connection=self.connection()
            connection.request('GET','/api/events?window='+('all' if index%2 else '24h'))
            response=connection.getresponse()
            self.assertEqual(response.status,200)
            self.event(response)
            response.close()
            connection.close()
        deadline=time.monotonic()+3
        slots=0
        while time.monotonic()<deadline:
            while self.dashboard.event_clients.acquire(False):
                slots+=1
            for _ in range(slots):self.dashboard.event_clients.release()
            if slots==8:
                break
            slots=0
            threading.Event().wait(.01)
        self.assertEqual(slots,8)
