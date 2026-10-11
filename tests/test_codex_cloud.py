import io
import json
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import MagicMock, patch
from urllib.parse import urlsplit

from local_activity_monitor.codex_cloud import CodexCloudSource
from local_activity_monitor.server import handler


class CodexCloudTests(unittest.TestCase):
    def test_disabled_missing_cli_and_invalid_cursor(self):
        source = CodexCloudSource(Path('/selected/home'))
        with patch('local_activity_monitor.codex_cloud._executable', return_value=None) as cli:
            self.assertEqual(source.query()['health'], 'disabled')
            cli.assert_not_called()
            for cursor in ('', 'x'*2049, '\n--unsafe', 3):
                with self.assertRaises(ValueError):
                    source.query(True, cursor)
            self.assertEqual(source.query(True)['health'], 'unavailable')

    def test_cache_is_per_cursor_and_copies_results(self):
        source = CodexCloudSource(Path('/selected/home'))
        with patch('local_activity_monitor.codex_cloud._executable', return_value='codex'), patch.object(source, '_read', return_value={'items': [{'id':'task_1'}], 'cursor':'next'} ) as read, patch('local_activity_monitor.codex_cloud.time.monotonic', return_value=0) as clock:
            source.query(True)['items'].clear()
            self.assertEqual(len(source.query(True)['items']), 1)
            source.query(True, 'next')
            self.assertEqual(read.call_count, 2)
            clock.return_value = 60
            read.side_effect = ValueError('private error')
            self.assertEqual(source.query(True)['health'], 'error')
            self.assertNotIn('private', json.dumps(source.query(True)))
            for index in range(12):
                source.query(True, str(index))
            self.assertEqual(len(source.cache), 10)

    def test_concurrent_queries_launch_once(self):
        source = CodexCloudSource(Path('/selected/home'))
        with patch('local_activity_monitor.codex_cloud._executable', return_value='codex'), patch.object(source, '_read', return_value={'items':[], 'cursor':None}) as read:
            with ThreadPoolExecutor(max_workers=8) as pool:
                values = list(pool.map(lambda _:source.query(True), range(8)))
        read.assert_called_once()
        self.assertTrue(all(value['health']=='ok' for value in values))

    def test_projection_keeps_metadata_without_body_url_or_account(self):
        value = {'tasks':[{'id':'task_demo', 'title':'Demo', 'status':'completed', 'updated_at':'2026-10-11T12:00:00Z', 'environment_label':'Demo environment', 'is_review':False, 'attempt_total':0, 'summary':'PRIVATE_MESSAGE', 'url':'PRIVATE_URL', 'email':'PRIVATE_ACCOUNT'}], 'cursor':'next_page'}
        result = CodexCloudSource._project(value)
        self.assertEqual(result['items'][0]['attempt_total'], 0)
        self.assertEqual(result['items'][0]['updated_at'], '2026-10-11T12:00:00.000Z')
        self.assertIs(result['items'][0]['is_review'], False)
        self.assertEqual(result['cursor'], 'next_page')
        self.assertNotIn('PRIVATE', json.dumps(result))
        for timestamp, expected in ((0,'1970-01-01T00:00:00.000Z'), ('bad',None), ('2026-10-11T12:00:00',None)):
            self.assertEqual(CodexCloudSource._project({'tasks':[{'id':'task','updated_at':timestamp}]})['items'][0]['updated_at'],expected)
        for value in ({}, {'tasks':{}}, {'tasks':[{'id':'../bad'}]}, {'tasks':[{'id':'task'}]*21}, {'tasks':[], 'cursor':'bad\n'}):
            with self.assertRaises(ValueError):
                CodexCloudSource._project(value)

    def run_cli(self, raw, code=0, alive=False):
        source = CodexCloudSource(Path('/selected/home'))
        process = MagicMock()
        process.stdout = io.BytesIO(raw)
        process.wait.return_value = code
        process.poll.return_value = None if alive else code
        with patch('local_activity_monitor.codex_cloud._executable', return_value='codex'), patch('local_activity_monitor.codex_cloud.subprocess.Popen', return_value=process) as launch:
            result = source.query(True, 'page_2')
        args, options = launch.call_args
        self.assertEqual(args[0], ['codex','cloud','list','--json','--limit','20','-c','analytics.enabled=false','--cursor','page_2'])
        self.assertEqual(options['env']['CODEX_HOME'], str(source.home))
        self.assertFalse(options['shell'])
        self.assertTrue(process.stdout.closed)
        return result, process

    def test_cli_readonly_command_and_cleanup(self):
        result, process = self.run_cli(b'{"tasks":[], "cursor":null}')
        self.assertEqual(result['health'], 'ok')
        process.terminate.assert_not_called()
        for raw, code in ((b'bad json',0), (b'{}',0), (b'x'*(1024*1024+1),0), (b'{}',1)):
            result, process = self.run_cli(raw, code, alive=True)
            self.assertEqual(result['health'], 'error')
            process.terminate.assert_called_once()

    def test_timeout_ends_owned_child(self):
        import queue
        with patch('local_activity_monitor.codex_cloud.queue.Queue.get', side_effect=queue.Empty):
            result, process = self.run_cli(b'{"tasks":[]}', alive=True)
        self.assertEqual(result['health'], 'error')
        process.terminate.assert_called_once()

    def test_http_validation_and_disabled_query(self):
        dashboard = MagicMock()
        dashboard.lock = threading.Lock()
        dashboard.observations = {'codex_cloud':False}
        dashboard.cloud = CodexCloudSource(Path('/selected/home'))
        from local_activity_monitor.api_queries import ApiQueries
        dashboard.api_queries = ApiQueries({'codex_cloud':lambda cursor:dashboard.cloud.query(True,cursor)},lambda:None)
        request = object.__new__(handler(dashboard, 8787))
        request.reply = MagicMock()
        with patch('local_activity_monitor.codex_cloud._executable') as cli:
            cli.return_value=None
            for query in ('cursor=x&cursor=y', 'url=https://example.test', 'cursor=', 'cursor=bad%0A', 'cursor='+'x'*4096):
                request.get_local(urlsplit('/api/codex/cloud?'+query))
                self.assertEqual(request.reply.call_args.args[0],400)
            request.get_local(urlsplit('/api/codex/cloud'))
            self.assertEqual(json.loads(request.reply.call_args.args[1])['health'],'unavailable')
            cli.assert_called_once()
