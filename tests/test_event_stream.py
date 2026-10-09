from http.client import HTTPConnection
from http.server import ThreadingHTTPServer
from pathlib import Path
import tempfile
import threading
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

    def test_initial_update_and_shutdown_release_stream_slot(self):
        connection=self.connection()
        connection.request('GET','/api/events')
        response=connection.getresponse()
        self.assertEqual(response.status,200)
        self.assertEqual(response.getheader('Content-Type'),'text/event-stream; charset=utf-8')
        self.assertEqual(response.readline(),b'retry: 3000\n')
        self.assertEqual(response.readline(),b'\n')
        self.assertEqual(response.readline(),b'id: 0\n')
        self.assertEqual(response.readline(),b'event: snapshot\n')
        self.assertEqual(response.readline(),b'data: {"version":0}\n')
        self.assertEqual(response.readline(),b'\n')
        self.dashboard.notify_update()
        self.assertEqual(response.readline(),b'id: 1\n')
        self.assertEqual(response.readline(),b'event: snapshot\n')
        self.assertEqual(response.readline(),b'data: {"version":1}\n')
        self.assertEqual(response.readline(),b'\n')
        self.dashboard.stop.set();self.dashboard.notify_update()
        self.assertEqual(response.read(),b'')
        self.assertTrue(self.dashboard.event_clients.acquire(False))
        self.dashboard.event_clients.release()

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
