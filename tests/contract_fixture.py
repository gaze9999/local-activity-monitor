"""Synthetic CODEX_HOME and output normalization for black-box HTTP contract tests.

Never reads real Codex data. All event times are offsets from an anchor that is
aligned to a minute and kept away from the 1h / 24h / 7d window boundaries, so
window membership and minute buckets are stable between runs.
"""
from contextlib import closing
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import re
import sqlite3

THREADS = {name: f'00000000-0000-4000-8000-{index:012d}' for index, name in enumerate(('main', 'worker', 'day', 'week', 'old', 'running', 'noisy', 'late'), start=1)}
SERVERS = {'demo_docs': '示範文件檢查', 'demo_search': '示範搜尋工具'}
CLOCK_MARGIN = timedelta(minutes=5)
RECORD_BYTES = 2048


def anchor_time():
    """Ten minutes before now, on a whole minute."""
    return datetime.now(timezone.utc).replace(second=0, microsecond=0)-timedelta(minutes=10)


def stamp(value):
    return value.isoformat(timespec='milliseconds').replace('+00:00', 'Z')


def fixed_line(text):
    line = text.encode('utf-8')
    if len(line) >= RECORD_BYTES:
        raise ValueError('Contract record exceeds its fixed byte budget')
    return line.ljust(RECORD_BYTES-1, b' ')+b'\n'


class SessionWriter:
    def __init__(self, path, anchor):
        self.path, self.anchor, self.records = path, anchor, []

    def add(self, kind, payload, seconds):
        self.records.append({'type': kind, 'timestamp': stamp(self.anchor+timedelta(seconds=seconds)), 'payload': payload})

    def call(self, name, arguments, output, seconds, call_id, duration=2, custom=False):
        kind = 'custom_tool_call' if custom else 'function_call'
        body = {'type': kind, 'name': name, 'call_id': call_id}
        body['input' if custom else 'arguments'] = arguments if isinstance(arguments, str) else json.dumps(arguments, ensure_ascii=False)
        self.add('response_item', body, seconds)
        if output is not None:
            self.add('response_item', {'type': kind+'_output', 'call_id': call_id, 'output': output if isinstance(output, str) else json.dumps(output, ensure_ascii=False)}, seconds+duration)

    def tokens(self, seconds, step, primary=28):
        usage = {'input_tokens': 1200*step, 'cached_input_tokens': 300*step, 'output_tokens': 400*step, 'reasoning_output_tokens': 100*step}
        usage['total_tokens'] = usage['input_tokens']+usage['output_tokens']
        limits = {'primary': {'used_percent': primary, 'window_minutes': 300, 'resets_at': int((self.anchor+timedelta(hours=3)).timestamp())},
                  'secondary': {'used_percent': 42, 'window_minutes': 10080, 'resets_at': int((self.anchor+timedelta(days=4)).timestamp())}, 'plan_type': 'pro'}
        self.add('event_msg', {'type': 'token_count', 'info': {'total_token_usage': usage, 'model_context_window': 258400}, 'rate_limits': limits}, seconds)

    def serialized(self):
        # Keep real paths while making byte offsets independent of root and OS.
        return b''.join(fixed_line(json.dumps(record, ensure_ascii=False)) for record in self.records)

    def write(self, extra_lines=(), mtime_offset=0):
        self.path.write_bytes(self.serialized()+b''.join(line.encode('utf-8')+b'\n' for line in extra_lines))
        when = (self.anchor+timedelta(seconds=mtime_offset)).timestamp()
        os.utime(self.path, (when, when))
        self.records = []

    def append(self):
        with self.path.open('ab') as stream:
            stream.write(self.serialized())
        self.records = []


def session_path(sessions, name):
    return sessions/f'rollout-2026-01-01T00-00-{list(THREADS).index(name):02}-{THREADS[name]}.jsonl'


def build(base, anchor):
    """Create the fixture under base; return the paths the tests need."""
    home, project = base/'home', base/'demo-project'
    sessions = home/'sessions'/'2026'/'01'/'01'
    sessions.mkdir(parents=True)
    project.mkdir()
    (project/'AGENTS.md').write_text('# Demo project\n\nSynthetic contract data only.\n', encoding='utf-8', newline='')
    skill = home/'skills'/'demo-skill'/'SKILL.md'
    skill.parent.mkdir(parents=True)
    skill.write_text('---\nname: demo-skill\ndescription: Synthetic skill\n---\n# Demo skill\n', encoding='utf-8', newline='')
    config = []
    for server, description in SERVERS.items():
        source = base/server
        source.mkdir()
        (source/'README.md').write_text('# Demo source\n', encoding='utf-8', newline='')
        (source/'package.json').write_text(json.dumps({'name': server, 'description': description}, ensure_ascii=False), encoding='utf-8', newline='')
        config.append(f'[mcp_servers.{server}]\ncommand = "node"\nargs = {json.dumps([str(source/"server.mjs")])}\n')
    (home/'config.toml').write_bytes(fixed_line('\n'.join(config)))
    (home/'.codex-global-state.json').write_text('{}', encoding='utf-8', newline='')

    def meta(writer, name, seconds, **extra):
        writer.add('session_meta', {'id': THREADS[name], 'originator': 'Codex Desktop', 'cwd': str(project), 'cli_version': '0.160.0'} | extra, seconds)

    def context(writer, model, effort, seconds):
        writer.add('turn_context', {'model': model, 'reasoning_effort': effort, 'approval_policy': 'on-request', 'sandbox_policy': {'mode': 'workspace-write', 'writable_roots': ['PRIVATE_ROOT']}}, seconds)

    # 1h window: one thread exercising every recognized operation family.
    main = SessionWriter(session_path(sessions, 'main'), anchor)
    meta(main, 'main', -2400, git={'branch': 'main', 'commit_hash': 'a'*40})
    context(main, 'demo-model-a', 'high', -2399)
    main.add('response_item', {'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': 'CONTRACT_PROMPT_PRIVATE'}]}, -2398)
    main.add('event_msg', {'type': 'task_started'}, -2398)
    main.tokens(-2390, 1)
    main.call('exec_command', {'cmd': 'git status', 'workdir': str(project)}, {'exit_code': 0, 'output': 'PRIVATE_STATUS'}, -2380, 'main-git')
    main.call('exec_command', {'cmd': 'git diff -- example.txt; python -m unittest tests.test_example'}, {'exit_code': 0, 'output': 'PRIVATE_DIFF'}, -2360, 'main-check', duration=5)
    main.call('exec_command', {'cmd': 'sqlite3 demo.db "SELECT 12; SELECT 34"'}, {'exit_code': 0, 'output': '12\n34'}, -2340, 'main-sql')
    main.call('exec_command', {'cmd': f'Get-Content "{skill}"'}, {'exit_code': 0, 'output': 'PRIVATE_SKILL_BODY'}, -2320, 'main-skill')
    main.call('apply_patch', '*** Begin Patch\n*** Add File: notes.txt\n+DETAIL_ONLY_PATCH_LINE\n*** End Patch\n', 'Success. Updated the following files:\nA notes.txt', -2300, 'main-patch', custom=True)
    main.call('mcp__demo_docs__inspect_document', {'source': 'C:/demo/report.pdf', 'ocr': 'auto', 'write': False},
              {'structuredContent': {'status': 'success', 'items_count': 3, 'latency_ms': 12, 'content_chars': 420, 'truncated': False}}, -2280, 'main-mcp', duration=3)
    main.call('web.run', {'search_query': [{'q': 'demo'}], 'open': [{'ref_id': 'https://example.com/docs?token=PRIVATE'}]},
              'See https://example.com/docs/page and https://example.org/x#frag', -2260, 'main-web')
    main.call('mcp__demo_search__search', {'query': '示範', 'limit': 5}, {'isError': True, 'content': [{'type': 'text', 'text': 'PRIVATE timeout'}]}, -2240, 'main-mcp-error', duration=7)
    main.call('functions.exec', 'text(await tools.exec_command({cmd:"git log -1"}));\ntext(await tools.mcp__demo_docs__inspect_document({source:"C:/demo/b.docx"}));',
              '{"exit_code":0}', -2220, 'main-nested', duration=4)
    main.call('collaboration.send_message', {'target': '/root/worker', 'message': 'PRIVATE_AGENT_MESSAGE'}, 'Message accepted', -2200, 'main-agent')
    main.call('exec_command', {'cmd': 'python -m pytest -q'}, {'exit_code': 1, 'output': 'PRIVATE_FAILURE'}, -2180, 'main-pytest', duration=9)
    main.call('mcp__demo_docs__inspect_document', {'source': 'C:/demo/pending.pdf'}, None, -2160, 'main-pending')
    main.tokens(-2150, 2)
    main.add('event_msg', {'type': 'task_complete'}, -2140)
    main.write(mtime_offset=-2140)

    worker = SessionWriter(session_path(sessions, 'worker'), anchor)
    meta(worker, 'worker', -2350, source={'subagent': {'thread_spawn': {'parent_thread_id': THREADS['main'], 'agent_path': '/root/worker'}}}, thread_source='subagent')
    context(worker, 'demo-model-b', 'medium', -2349)
    worker.add('response_item', {'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': 'Message Type: MESSAGE\nTask name: /root/worker\nSender: /root\nPayload:\nPRIVATE_RECEIVED'}]}, -2348)
    worker.add('event_msg', {'type': 'task_started'}, -2348)
    worker.call('mcp__demo_search__search', {'query': 'worker'}, {'structuredContent': {'status': 'success', 'items_count': 1}}, -2340, 'worker-mcp')
    worker.add('event_msg', {'type': 'turn_failed', 'error_code': 'demo_timeout'}, -2330)
    worker.write(mtime_offset=-2330)

    running = SessionWriter(session_path(sessions, 'running'), anchor)
    meta(running, 'running', -1800)
    context(running, 'demo-model-a', 'low', -1799)
    running.add('event_msg', {'type': 'task_started'}, -1798)
    running.call('exec_command', {'cmd': 'git fetch origin main'}, None, -1790, 'running-git')
    running.write(mtime_offset=-1790)

    noisy = SessionWriter(session_path(sessions, 'noisy'), anchor)
    meta(noisy, 'noisy', -1500)
    noisy.add('unknown_kind', {'value': 1}, -1499)
    noisy.add('event_msg', {'type': 'task_started'}, -1498)
    noisy.call('mcp__demo_docs__inspect_document', {'source': 'C:/demo/c.md'}, {'status': 'ok', 'items_count': 0}, -1490, 'noisy-mcp')
    noisy.add('event_msg', {'type': 'error', 'error': {'code': 'demo_stream_error'}}, -1480)
    noisy.add('event_msg', {'type': 'task_complete'}, -1470)
    noisy.write(extra_lines=['{"type": "response_item", "payload": ', 'not json at all'], mtime_offset=-1470)

    # 24h, 7d and all-only windows, far from every boundary. SQLite aggregate
    # buckets widen past a 24h span and align to the epoch, not to the anchor,
    # so MCP / SQL history stays inside 24h and older threads use other tools.
    for name, offset, model in (('day', -6*3600, 'demo-model-b'), ('week', -3*86400, 'demo-model-c'), ('old', -20*86400, 'demo-model-c')):
        writer = SessionWriter(session_path(sessions, name), anchor)
        meta(writer, name, offset)
        context(writer, model, 'medium', offset+1)
        writer.add('event_msg', {'type': 'task_started'}, offset+2)
        writer.tokens(offset+5, 3, primary=10)
        for index in range(3):
            if name == 'day':
                server = tuple(SERVERS)[index % 2]
                tool = 'inspect_document' if server == 'demo_docs' else 'search'
                writer.call(f'mcp__{server}__{tool}', {'query': f'{name}-{index}'}, {'structuredContent': {'status': 'success', 'items_count': index}}, offset+10+index*30, f'{name}-mcp-{index}', duration=index+1)
            elif index == 0:
                writer.call('web.run', {'search_query': [{'q': name}]}, f'Result https://example.net/{name}', offset+10, f'{name}-web')
            else:
                writer.call('apply_patch', f'*** Begin Patch\n*** Update File: {name}-{index}.txt\n@@\n-old\n+PRIVATE_NEW\n*** End Patch\n', 'Success', offset+10+index*30, f'{name}-patch-{index}', duration=index, custom=True)
        writer.call('exec_command', {'cmd': 'git commit -m "demo"'}, {'exit_code': 0, 'output': 'PRIVATE'}, offset+120, f'{name}-git')
        writer.add('event_msg', {'type': 'task_complete'}, offset+200)
        writer.write(mtime_offset=offset+200)

    with closing(sqlite3.connect(home/'state_5.sqlite')) as db, db:
        db.executescript('CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT,archived); CREATE TABLE projects(id TEXT,name TEXT); CREATE TABLE project_roots(project_id TEXT,path TEXT);')
        db.executemany('INSERT INTO threads VALUES(?,?,?,?)', [(identity, '示範 '+name, 'demo-project', 1 if name == 'old' else 0) for name, identity in THREADS.items()])
        db.execute('INSERT INTO projects VALUES(?,?)', ('demo-project', '示範專案'))
        db.execute('INSERT INTO project_roots VALUES(?,?)', ('demo-project', str(project)))
    with closing(sqlite3.connect(home/'logs_10.sqlite')) as db, db:
        db.execute('CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,feedback_log_body TEXT)')
        moment = lambda seconds: (anchor+timedelta(seconds=seconds)).timestamp()
        db.executemany('INSERT INTO logs VALUES(?,?,?,?,?)', [
            (1, moment(-2500), 'INFO', 'demo_docs', 'Synthetic demo log only'),
            (2, moment(-2400), 'WARN', 'codex_core::responses_retry', 'stream disconnected retries=1 model=demo-model-b'),
            (3, moment(-2300), 'INFO', 'codex_otel.log_only', f'event.name="codex.api_request" endpoint="/responses" model=demo-model-a provider=OpenAI status_code=200 duration_ms=40 input_tokens=10 output_tokens=8 request_id=req_demo thread_id={THREADS["main"]}'),
            (4, moment(-2200), 'ERROR', 'codex_otel.log_only', 'event.name="codex.api_request" endpoint="/responses" model=demo-model-c status_code=429 request_id=req_failed errorMessage="PRIVATE"'),
            (5, moment(-7*3600), 'ERROR', 'codex_core::exec', 'sqlite error: database is locked PRIVATE_SQL SELECT secret FROM t'),
        ])
    return {'home': home, 'project': project, 'sessions': sessions, 'skill': skill}


def append_activity(paths, anchor):
    """New calls in an existing session and one new session, both after the initial data."""
    main = SessionWriter(session_path(paths['sessions'], 'main'), anchor)
    main.add('event_msg', {'type': 'task_started'}, 60)
    main.call('mcp__demo_search__search', {'query': 'appended'}, {'structuredContent': {'status': 'success', 'items_count': 7, 'input_tokens': 5}}, 62, 'main-appended-mcp')
    main.call('exec_command', {'cmd': 'git push origin main'}, {'exit_code': 0, 'output': 'PRIVATE'}, 70, 'main-appended-git')
    main.tokens(80, 3)
    main.add('event_msg', {'type': 'task_complete'}, 90)
    main.append()
    late = SessionWriter(session_path(paths['sessions'], 'late'), anchor)
    late.add('session_meta', {'id': THREADS['late'], 'originator': 'Codex Desktop', 'cwd': str(paths['project'])}, 30)
    late.add('event_msg', {'type': 'task_started'}, 31)
    late.call('mcp__demo_docs__inspect_document', {'source': 'C:/demo/late.pdf'}, {'structuredContent': {'status': 'success', 'items_count': 2}}, 40, 'late-mcp')
    late.add('event_msg', {'type': 'task_complete'}, 50)
    late.write(mtime_offset=50)


def without_program_journal(logs):
    """Drop the service's own lifecycle journal, which differs per process and shutdown race.

    Accepts a /api/logs body or a snapshot's logs object; the journal source keeps
    its health, limits and file name, and only its entries and counters are removed.
    """
    logs = dict(logs)
    journal = next((source for source in logs.get('sources', []) if source.get('source') == 'monitor'), None)
    if journal is not None:
        logs['sources'] = [{key: value for key, value in source.items() if key not in ('bytes', 'retained', 'checked_at')} if source is journal else source for source in logs['sources']]
        if 'retained' in logs and isinstance(journal.get('retained'), int):
            logs['retained'] -= journal['retained']
    if 'entries' in logs:
        kept = [entry for entry in logs['entries'] if entry.get('source') != 'monitor']
        if 'total' in logs:
            logs['total'] -= len(logs['entries'])-len(kept)
        logs['entries'] = kept
    return logs


ISO = re.compile(r'\d{4}-\d\d-\d\dT\d\d:\d\d(?::\d\d(?:\.\d+)?)?(?:Z|[+-]\d\d:\d\d)?')
HEX = re.compile(r'[0-9a-f]{64}')


class Normalizer:
    """Map run-specific values to stable tokens while keeping their relationships."""
    VOLATILE = {'monitor', 'activity', 'revision', 'frontend_revision', 'backend_revision', 'started_at', 'code_revision'}

    def __init__(self, anchor, paths):
        self.anchor = anchor
        self.horizon = anchor+CLOCK_MARGIN
        self.paths = sorted(((str(value), '<'+key.upper()+'>') for key, value in paths.items() if isinstance(value, Path)), key=lambda item: -len(item[0]))
        self.hashes = {}

    def time(self, text):
        try:
            value = datetime.fromisoformat(text.replace('Z', '+00:00'))
        except ValueError:
            return text
        if value.tzinfo is None:
            return text
        if value > self.horizon:
            return '<clock>'
        return f'T{int((value-self.anchor).total_seconds()*1000):+d}ms'

    def text(self, value):
        for raw, token in self.paths:
            for variant in (raw, raw.replace('\\', '/'), raw.replace('\\', '\\\\')):
                value = value.replace(variant, token)
        value = ISO.sub(lambda match: self.time(match[0]), value)
        return HEX.sub(lambda match: self.hashes.setdefault(match[0], f'<sha:{len(self.hashes)+1}>'), value)

    def __call__(self, value, top=True):
        if isinstance(value, dict):
            result = {self.text(key): self(item, False) for key, item in sorted(value.items()) if not (top and key in self.VOLATILE)}
            if value.get('status') == 'running' and 'task_duration_ms' in value:
                # A running task's duration is measured up to the current clock.
                result['task_duration_ms'] = '<running>'
            return result
        if isinstance(value, list):
            stamps = [item.get('timestamp') if isinstance(item, dict) else None for item in value]
            if len(value) > 1 and all(isinstance(item, str) for item in stamps) and stamps == sorted(stamps, reverse=True):
                # Lists already ordered by time may order equal times by a time-dependent hash.
                # Entries stamped by the service clock (program events) form one group.
                value = sorted(value, key=lambda item: ISO.sub('', HEX.sub('', json.dumps(item, sort_keys=True, ensure_ascii=False))))
                value.sort(key=lambda item: '~clock' if self.time(item['timestamp']) == '<clock>' else item['timestamp'], reverse=True)
            return [self(item, False) for item in value]
        if isinstance(value, str):
            return self.text(value)
        if isinstance(value, (int, float)) and not isinstance(value, bool) and 1e9 < value < 1e10:
            # Epoch seconds (for example rate-limit reset times) follow the same anchor rule.
            return self.time(datetime.fromtimestamp(value, timezone.utc).isoformat())
        return value
