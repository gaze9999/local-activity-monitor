"""Black-box HTTP / SSE contract for refactors or another implementation.

The service runs as a separate process on a random loopback port with an
isolated synthetic CODEX_HOME. Assertions use only HTTP responses, never
internal functions. LAM_CONTRACT_COMMAND (a JSON list) replaces the server
command; it receives --codex-home, --port 0 and --codex, prints the usual
listening line, and stops when stdin closes. LAM_UPDATE_CONTRACT=1 rewrites
the golden files after an intended behavior change.
"""
from datetime import timedelta
import gzip
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.client import HTTPConnection

from contract_fixture import HEX, THREADS, Normalizer, anchor_time, append_activity, build, without_program_journal

ROOT = Path(__file__).resolve().parents[1]
GOLDEN = Path(__file__).resolve().parent/'contract_golden'
WINDOWS = ('1h', '24h', '7d', 'all')
DEFAULT_COMMAND = [sys.executable, '-X', 'utf8', '-c',
                   'import sys; from local_activity_monitor.server import main; raise SystemExit(main(sys.argv[1:], watch_stdin=True))']


class Service:
    def __init__(self, home):
        command = json.loads(os.environ['LAM_CONTRACT_COMMAND']) if os.environ.get('LAM_CONTRACT_COMMAND') else DEFAULT_COMMAND
        environment = {key: value for key, value in os.environ.items() if key != 'CODEX_HOME'}
        environment['PYTHONPATH'] = os.pathsep.join(filter(None, (str(ROOT/'src'), environment.get('PYTHONPATH'))))
        environment['PYTHONDONTWRITEBYTECODE'] = '1'
        self.process = subprocess.Popen(command+['--codex-home', str(home), '--port', '0', '--codex'], cwd=ROOT, env=environment,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8')
        self.errors = []
        threading.Thread(target=lambda: self.errors.extend(self.process.stderr), daemon=True).start()
        line = self.process.stdout.readline()
        if not line:
            self.process.wait(10)
            raise RuntimeError('Contract service did not start: '+''.join(self.errors)[-2000:])
        status = json.loads(line)
        if status.get('status') != 'listening':
            raise RuntimeError(f'Unexpected service status: {status}')
        self.port = int(status['url'].rstrip('/').rsplit(':', 1)[1])

    def request(self, path, headers=None):
        connection = HTTPConnection('127.0.0.1', self.port, timeout=20)
        try:
            connection.request('GET', path, headers=headers or {})
            response = connection.getresponse()
            body = response.read()
            if response.getheader('Content-Encoding') == 'gzip':
                body = gzip.decompress(body)
            return response.status, body
        finally:
            connection.close()

    def json(self, path):
        status, body = self.request(path)
        if status != 200:
            raise AssertionError(f'{path} returned {status}: {body[:200]!r}')
        return json.loads(body)

    def first_frame(self, window):
        connection = HTTPConnection('127.0.0.1', self.port, timeout=20)
        try:
            connection.request('GET', '/api/events?window='+window)
            response = connection.getresponse()
            lines = [response.readline() for _ in range(4)]
            assert lines[1] == b'event: snapshot\n' and lines[2].startswith(b'data: ') and lines[3] == b'\n', lines[:2]
            frame = json.loads(lines[2][6:])
            assert lines[0] == f'id: {frame["version"]}\n'.encode()
            return frame
        finally:
            connection.close()

    def settle(self, condition, timeout=60):
        """Wait for collection to finish and the published snapshot to stop changing."""
        deadline, previous, stable_since = time.monotonic()+timeout, None, None
        while time.monotonic() < deadline:
            snapshot = self.json('/api/snapshot?window=all')
            state = snapshot.get('codex', {}).get('read_state', {})
            ready = snapshot.get('updated_at') and snapshot['monitor'].get('collection', {}).get('phase') == 'idle' and condition(snapshot) and not any(
                state.get(key) for key in ('pending_files', 'initial_pending_files', 'history_pending_files', 'error_pending_files'))
            marker = (snapshot.get('updated_at'), snapshot['activity'].get('last_activity_at'))
            if ready and marker == previous:
                if stable_since and time.monotonic()-stable_since >= 1.5:
                    return snapshot
                stable_since = stable_since or time.monotonic()
            else:
                stable_since = None
            previous = marker
            time.sleep(.25)
        raise TimeoutError('Contract service did not settle')

    def stop(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(20)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(10)
        self.process.stdout.close()


def tool_calls(snapshot):
    return sum(row.get('tool_calls') or 0 for row in snapshot['codex']['threads'])


class HttpContractTests(unittest.TestCase):
    maxDiff = None

    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='lam-contract-')
        cls.anchor = anchor_time()
        cls.paths = build(Path(cls.temp.name), cls.anchor)
        cls.services = []

    @classmethod
    def tearDownClass(cls):
        for service in cls.services:
            service.stop()
        cls.temp.cleanup()

    def start(self):
        service = Service(self.paths['home'])
        self.services.append(service)
        return service

    def capture(self, service):
        normalize = Normalizer(self.anchor, self.paths)
        result = {'snapshot': {}, 'logs': {}, 'history': {}}
        def stable(snapshot):
            return snapshot | {'logs': without_program_journal(snapshot['logs'])} if 'logs' in snapshot else snapshot
        for window in WINDOWS:
            snapshot = stable(service.json('/api/snapshot?window='+window))
            logs = without_program_journal(service.json('/api/logs?window='+window))
            frame = service.first_frame(window)
            self.assertEqual(frame['window'], window)
            # One SSE frame carries the same projection as the HTTP endpoints.
            self.assertEqual(Normalizer(self.anchor, self.paths)(stable(frame['snapshot'])), Normalizer(self.anchor, self.paths)(snapshot))
            self.assertEqual(Normalizer(self.anchor, self.paths)(without_program_journal(frame['logs'])), Normalizer(self.anchor, self.paths)(logs))
            result['snapshot'][window] = normalize(snapshot)
            result['logs'][window] = normalize(logs)
        for namespace, section in (('activity', 'mcp'), ('activity', 'web'), ('activity', 'sql'), ('errors', 'events'), ('threads', 'skills'), ('threads', 'entries')):
            listing = service.json(f'/api/history?namespace={namespace}&section={section}&limit=200')
            self.assertIsNone(listing['next_cursor'])
            items = listing['items']
            stamps = [item.get('timestamp') or item.get('task_time') or '' for item in items]
            self.assertEqual(stamps, sorted(stamps, reverse=True), f'{namespace}/{section} order')
            # Cursor pages together return the same rows once each.
            paged, cursor = [], None
            for _ in range(100):
                page = service.json(f'/api/history?namespace={namespace}&section={section}&limit=3'+(f'&cursor={cursor}' if cursor else ''))
                self.assertEqual(page['total'], listing['total'])
                paged.extend(page['items'])
                cursor = page['next_cursor']
                if not cursor:
                    break
            self.assertEqual(sorted(json.dumps(item, sort_keys=True) for item in paged), sorted(json.dumps(item, sort_keys=True) for item in items))
            # Rows with equal times are ordered by a time-dependent hash, so the golden uses a canonical tie order.
            items.sort(key=lambda item: HEX.sub('', json.dumps(item, sort_keys=True)))
            items.sort(key=lambda item: item.get('timestamp') or item.get('task_time') or '', reverse=True)
            result['history'][f'{namespace}/{section}'] = normalize(listing | {'items': items})
        result['details'] = {
            'tool': normalize(service.json(f'/api/codex/tool?thread_id={THREADS["main"]}&call_id=main-patch')),
            'mcp': normalize(service.json(f'/api/codex/mcp?thread_id={THREADS["main"]}&call_id=main-mcp&index=0')),
        }
        return result

    def compare(self, name, value):
        path = GOLDEN/f'{name}.json'
        text = json.dumps(value, ensure_ascii=False, indent=1, sort_keys=True)+'\n'
        if os.environ.get('LAM_UPDATE_CONTRACT') == '1':
            GOLDEN.mkdir(exist_ok=True)
            path.write_text(text, encoding='utf-8', newline='\n')
            return
        if not path.is_file():
            self.fail(f'Missing golden file {path.name}; run with LAM_UPDATE_CONTRACT=1 after reviewing the output')
        expected = json.loads(path.read_text(encoding='utf-8'))
        for section in expected:
            with self.subTest(golden=name, section=section):
                self.assertEqual(value.get(section), expected[section])
        self.assertEqual(sorted(value), sorted(expected))

    def assert_private_values_absent(self, captured):
        raw = json.dumps(captured, ensure_ascii=False)
        for secret in ('CONTRACT_PROMPT_PRIVATE', 'PRIVATE_STATUS', 'PRIVATE_DIFF', 'PRIVATE_SKILL_BODY', 'PRIVATE_AGENT_MESSAGE', 'PRIVATE_RECEIVED',
                       'PRIVATE_FAILURE', 'PRIVATE_ROOT', 'PRIVATE_SQL', 'token=PRIVATE', 'PRIVATE timeout', 'DETAIL_ONLY_PATCH_LINE'):
            self.assertNotIn(secret, raw)

    def test_lifecycle_contract(self):
        service = self.start()
        initial = service.settle(lambda snapshot: tool_calls(snapshot) > 0)
        before = self.capture(service)
        self.assert_private_values_absent({key: before[key] for key in ('snapshot', 'logs', 'history')})
        # Patch bodies are available only from the on-demand detail endpoint.
        self.assertIn('DETAIL_ONLY_PATCH_LINE', before['details']['tool']['request'])
        self.compare('initial', before)

        append_activity(self.paths, self.anchor)
        version = service.first_frame('all')['version']
        service.settle(lambda snapshot: tool_calls(snapshot) == tool_calls(initial)+3)
        self.assertGreater(service.first_frame('all')['version'], version)
        appended = self.capture(service)
        self.compare('appended', appended)

        service.stop()
        restarted_service = self.start()
        restarted_service.settle(lambda snapshot: tool_calls(snapshot) == tool_calls(initial)+3)
        restarted = self.capture(restarted_service)
        self.compare('restarted', restarted)
        restarted_service.stop()

    def test_request_guards(self):
        service = self.start()
        service.settle(lambda snapshot: tool_calls(snapshot) > 0)
        for path, status in (('/api/snapshot?window=bad', 400), ('/api/snapshot?window=24h&extra=1', 400), ('/api/logs?window=bad', 400),
                             ('/api/events?window=bad', 400), ('/api/history?namespace=activity&section=bad', 400),
                             (f'/api/codex/tool?thread_id={THREADS["main"]}&call_id=missing', 409), ('/api/codex/tool?thread_id=bad&call_id=x', 400)):
            with self.subTest(path=path):
                self.assertEqual(service.request(path)[0], status)
        self.assertEqual(service.request('/api/snapshot?window=24h', {'Host': 'evil.invalid'})[0], 403)
        self.assertEqual(service.request('/api/snapshot?window=24h', {'Origin': 'http://evil.invalid'})[0], 403)
        status, body = service.request('/api/snapshot?window=24h', {'Accept-Encoding': 'gzip'})
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body)['codex']['source'], 'codex')
        service.stop()


if __name__ == '__main__':
    unittest.main()
