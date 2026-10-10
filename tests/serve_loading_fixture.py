"""Serve isolated synthetic data for loading and layout checks. Never read real Codex data."""
from contextlib import closing
from datetime import datetime, timedelta, timezone
from http.server import ThreadingHTTPServer
import argparse
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import threading

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--startup-delay', type=float, default=0)
    options = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="lam-loading-fixture-") as folder:
        root = Path(__file__).resolve().parents[1]
        base = Path(folder)
        home = base / 'home'
        sessions = home / 'sessions'
        sessions.mkdir(parents=True)
        project = base / 'demo-project'
        project.mkdir()
        (project / 'AGENTS.md').write_text('# Demo project\n\nSynthetic screenshot data only.\n', encoding='utf-8')
        anchor = datetime.now(timezone.utc).replace(microsecond=0) - timedelta(minutes=2)

        def stamp(value):
            return value.isoformat().replace('+00:00', 'Z')

        servers = {'demo_docs': '示範文件檢查', 'demo_search': '示範搜尋工具', 'demo_checks': '示範驗證工具'}
        config = []
        for name, description in servers.items():
            source = base / name
            source.mkdir()
            (source / 'server.mjs').write_text('// Inert demo file; never executed.\n', encoding='utf-8')
            (source / 'package.json').write_text(json.dumps({'name': name, 'description': description}, ensure_ascii=False), encoding='utf-8')
            config.append(f'[mcp_servers.{name}]\ncommand="node"\nargs={json.dumps([str(source / "server.mjs")])}\n')
        (home / 'config.toml').write_text('\n'.join(config), encoding='utf-8')
        titles = ['示範介面整理', '示範工具統計', '示範文件維護', '示範資料讀取', '示範專案導覽', '示範錯誤分類', '示範格式預覽', '示範 MCP 比較', '示範功能檢查', '示範版本資訊', '示範圖表設定', '示範效能觀察']
        threads = []
        for index, title in enumerate(titles):
            identity = f'00000000-0000-4000-8000-{index + 1:012d}'
            began = anchor - timedelta(hours=11-index/2)
            records = []
            def add(kind, payload, when):
                records.append({'type': kind, 'timestamp': stamp(when), 'payload': payload})
            add('session_meta', {'id': identity, 'originator': 'Codex Desktop', 'cwd': str(project), 'cli_version': '0.160.0'}, began)
            if index == 1:
                records[0]['payload']['source'] = {'subagent':{'thread_spawn':{'parent_thread_id':'00000000-0000-4000-8000-000000000001','agent_path':'/root/worker'}}}
            add('response_item',{'type':'message','role':'user','content':[{'type':'input_text','text':'PARENT_CONTEXT_ONLY' if index == 0 else 'CHILD_CONTEXT_ONLY' if index == 1 else 'DEMO_CONTEXT'}]},began)
            if index == 0:
                add('response_item',{'type':'function_call','name':'collaboration.send_message','call_id':'demo_agent_parent','arguments':json.dumps({'target':'/root/worker','message':'PARENT_SENT_ONLY'})},began+timedelta(seconds=1))
                add('response_item',{'type':'function_call_output','call_id':'demo_agent_parent','output':'Parent message accepted'},began+timedelta(seconds=2))
            elif index == 1:
                add('response_item',{'type':'message','role':'user','content':[{'type':'input_text','text':'Message Type: MESSAGE\nTask name: /root/worker\nSender: /root\nPayload:\nCHILD_RECEIVED_ONLY'}]},began+timedelta(seconds=1))
            add('turn_context', {'model': ['demo-model-a', 'demo-model-b', 'demo-model-c'][index % 3], 'reasoning_effort': ['high', 'medium', 'low'][index % 3]}, began)
            add('event_msg', {'type': 'task_started'}, began)
            for step in range(8):
                when = began + timedelta(minutes=step * (18 + index))
                tokens = {'input_tokens': (step+1)*(1200+index*140), 'cached_input_tokens': (step+1)*350, 'output_tokens': (step+1)*(480+index*60), 'reasoning_output_tokens': (step+1)*120}
                tokens['total_tokens'] = tokens['input_tokens'] + tokens['output_tokens']
                add('event_msg', {'type': 'token_count', 'info': {'total_token_usage': tokens}, 'rate_limits': {'primary': {'used_percent': 28, 'window_minutes': 300, 'resets_at': (anchor+timedelta(hours=3)).timestamp()}, 'secondary': {'used_percent': 42, 'window_minutes': 10080, 'resets_at': (anchor+timedelta(days=4)).timestamp()}, 'plan_type': 'pro'}}, when)
                tools = ['exec_command', 'apply_patch', 'mcp__demo_docs__inspect_document', 'mcp__demo_search__search', 'mcp__demo_checks__validate']
                for offset in range(2 + (step+index) % 3):
                    tool = tools[(step+index+offset) % len(tools)]
                    call = f'demo_{index}_{step}_{offset}'
                    started = when + timedelta(seconds=offset*10)
                    duration = .2 + ((index+step+offset) % 7)*.35
                    arguments = {'query': '示範文件', 'limit': 5} if tool.startswith('mcp__') else {'cmd': 'echo demo'} if tool == 'exec_command' else {'patch': 'demo'}
                    add('response_item', {'type': 'function_call', 'name': tool, 'call_id': call, 'arguments': json.dumps(arguments, ensure_ascii=False)}, started)
                    output = {'status': 'success', 'items_count': 5}
                    if (index+step+offset) % 19 == 0:
                        output = {'isError': True, 'error': {'type': 'demo_timeout', 'message': '示範逾時, 用於呈現錯誤分類'}}
                    add('response_item', {'type': 'function_call_output', 'call_id': call, 'output': json.dumps(output, ensure_ascii=False)}, started+timedelta(seconds=duration))
            ended = began+timedelta(minutes=8*(18+index))
            if index in (1, 6, 9):
                add('event_msg', {'type': 'turn_failed', 'error_code': ['demo_timeout', 'demo_validation', 'demo_connection'][index % 3]}, ended)
            elif index not in (0, 4):
                add('event_msg', {'type': 'task_complete'}, ended)
            else:
                add('event_msg', {'type': 'task_started'}, anchor-timedelta(seconds=index*10))
            (sessions / f'rollout-{identity}.jsonl').write_text('\n'.join(json.dumps(record, ensure_ascii=False) for record in records)+'\n', encoding='utf-8')
            threads.append((identity, title, 'demo-project', 0))
        with closing(sqlite3.connect(home / 'state_5.sqlite')) as db, db:
            db.executescript('CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT,archived); CREATE TABLE projects(id TEXT,name TEXT); CREATE TABLE project_roots(project_id TEXT,path TEXT);')
            db.executemany('INSERT INTO threads VALUES(?,?,?,?)', threads)
            db.execute('INSERT INTO projects VALUES(?,?)', ('demo-project', '示範專案'))
            db.execute('INSERT INTO project_roots VALUES(?,?)', ('demo-project', str(project)))
        with closing(sqlite3.connect(home / 'logs_10.sqlite')) as db, db:
            db.execute('CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,feedback_log_body TEXT)')
            db.executemany('INSERT INTO logs VALUES(?,?,?,?,?)', [(index+1, (anchor-timedelta(minutes=index*12)).timestamp(), ['INFO', 'WARN', 'ERROR'][index%3], ['demo_docs', 'demo_search', 'demo_checks'][index%3], 'Synthetic demo log only') for index in range(12)])
            db.executemany('INSERT INTO logs VALUES(?,?,?,?,?)', [
                (20, anchor.timestamp(), 'INFO', 'codex_otel.log_only', 'event.name="codex.api_request" endpoint="/responses" model=demo-model-a provider=OpenAI status_code=200 duration_ms=0 input_tokens=0 output_tokens=8 request_id=req_demo thread_id=00000000-0000-4000-8000-000000000001'),
                (21, anchor.timestamp(), 'INFO', 'codex_otel.log_only', 'event.name="codex.websocket.request" model=demo-model-b success="true" duration_ms=3'),
                (22, anchor.timestamp(), 'INFO', 'codex_api::endpoint::responses_websocket', 'successfully connected to websocket: ws://synthetic.invalid model=demo-model-a provider=OpenAI transport="responses_websocket"'),
                (23, anchor.timestamp(), 'WARN', 'codex_core::responses_retry', 'stream disconnected retries=1 model=demo-model-b'),
                (24, anchor.timestamp(), 'ERROR', 'codex_otel.log_only', 'event.name="codex.api_request" endpoint="/responses" model=demo-model-c status_code=429 request_id=req_demo_failed errorMessage="PRIVATE"'),
            ])

        sys.path.insert(0, str(root / 'src'))
        from local_activity_monitor.server import Dashboard, handler
        from local_activity_monitor import __version__
        dashboard = Dashboard(home.resolve(), codex=True, max_files=20)
        dashboard.observations['worktrees'] = False
        dashboard.monitor.load_device = lambda: None
        dashboard.account.snapshot = lambda enabled=False: {'health': 'disabled', 'limits': {}, 'account_usage': None, 'methods': {name: {'health': 'disabled'} for name in ('account/read', 'account/rateLimits/read', 'account/usage/read')}}
        dashboard.started_at = stamp(anchor-timedelta(hours=2))
        def collect():
            dashboard.refresh()
            for snapshot in dashboard.cache.values():
                snapshot['updated_at'] = stamp(anchor)
        if options.startup_delay:
            dashboard.preload()
        else:
            collect()
        original_snapshot = dashboard.snapshot
        def demo_snapshot(window='all'):
            value = original_snapshot(window)
            if value['codex'].get('usage'):
                value['codex']['usage']['credits'] = {'balance': 0, 'has_credits': False, 'unlimited': False}
            value['monitor'] = {'log_path': value['monitor'].get('log_path'), 'debug': value['monitor'].get('debug', {}), 'version': __version__, 'uptime_seconds': 7200, 'health': 'ok', 'requests': 24, 'errors': 0, 'collection': {'phase': 'idle'}, 'device_ready': True, 'gpus':[{'name':'Demo single GPU','dedicated_memory_bytes':8*1024**3,'shared_memory_bytes':16*1024**3,'driver_version':'demo'}], 'fonts':['Segoe UI','Microsoft JhengHei'], 'history': [], 'events': []}
            return value
        dashboard.snapshot = demo_snapshot
        server = ThreadingHTTPServer(('127.0.0.1', 0), handler(dashboard, 0))
        server.RequestHandlerClass = handler(dashboard, server.server_port)
        (base / 'service.json').write_text(json.dumps({'port': server.server_port, 'synthetic': True}), encoding='utf-8')
        print(json.dumps({'url': f'http://127.0.0.1:{server.server_port}/', 'synthetic': True, 'threads': len(threads)}), flush=True)
        worker = None
        if options.startup_delay:
            def delayed_collect():
                if not dashboard.stop.wait(options.startup_delay):
                    collect()
            worker = threading.Thread(target=delayed_collect, daemon=True)
            worker.start()
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            dashboard.stop.set()
            dashboard.notify_update()
            server.server_close()
            if worker:
                worker.join(timeout=5)


if __name__ == "__main__":
    main()
