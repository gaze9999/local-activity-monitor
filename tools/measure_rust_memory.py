"""Linux synthetic Rust-service memory sample; loopback only, fresh homes/processes."""
import argparse
import datetime, hashlib, http.client, json, os, pathlib, selectors, subprocess, tempfile, time
from urllib.parse import urlsplit
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', required=True, type=pathlib.Path, help='Already built Rust service executable')
args = parser.parse_args()
if not pathlib.Path('/proc/self/status').is_file():
    parser.error('This measurement requires Linux /proc')
binary = args.binary.resolve(strict=True)
results = []
for count in (0, 1000, 5000):
    for repeat in (1, 2):
        with tempfile.TemporaryDirectory(prefix='lam-memory-') as scratch:
            home = pathlib.Path(scratch)
            sessions = home / 'sessions'
            sessions.mkdir()
            stamp = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='milliseconds').replace('+00:00', 'Z')
            source_bytes = 0
            for first in range(0, count, 500):
                uid = f'00000000-0000-4000-8000-{first // 500 + 1:012d}'

                def event(kind, payload):
                    return json.dumps(dict(type=kind, timestamp=stamp, payload=payload), separators=(',', ':')) + '\n'
                rows = [event('session_meta', dict(id=uid))]
                for n in range(first, min(first + 500, count)):
                    rows.extend([event('response_item', dict(type='function_call', name='fixture_tool', call_id=f'call{n}', arguments=json.dumps({'value': 'x' * 64}))), event('response_item', dict(type='function_call_output', call_id=f'call{n}', output='fixture complete'))])
                data = ''.join(rows).encode()
                source_bytes += len(data)
                (sessions / f'rollout-{uid}.jsonl').write_bytes(data)
            env = {**os.environ, 'HOME': scratch, 'CODEX_HOME': scratch, 'XDG_DATA_HOME': scratch, 'LOCALAPPDATA': scratch}
            with tempfile.TemporaryFile() as err:
                start = time.monotonic()
                p = subprocess.Popen([str(binary), '--port', '0', '--watch-stdin', '--codex-home', scratch, '--codex'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, env=env)
                try:
                    with selectors.DefaultSelector() as sel:
                        sel.register(p.stdout, selectors.EVENT_READ)
                        assert sel.select(10), 'startup timeout'
                    ready = json.loads(p.stdout.readline())
                    assert ready['status'] == 'listening'
                    url = urlsplit(ready['url'])
                    assert url.hostname == '127.0.0.1'
                    deadline = time.monotonic() + 30
                    requests = 0
                    while True:
                        c = http.client.HTTPConnection('127.0.0.1', url.port, timeout=5)
                        try:
                            c.request('GET', '/api/snapshot?window=all')
                            r = c.getresponse()
                            body = r.read()
                            assert r.status == 200
                        finally:
                            c.close()
                        requests += 1
                        v = json.loads(body)
                        if v['codex'].get('observed_tool_calls') == count and v['monitor']['device_ready'] and (v['monitor']['collection']['phase'] == 'idle'):
                            break
                        assert time.monotonic() < deadline, ('collection timeout', count, v['codex'].get('observed_tool_calls'))
                        time.sleep(0.05)
                    metrics = {}
                    for line in pathlib.Path(f'/proc/{p.pid}/status').read_text().splitlines():
                        if line.startswith(('VmRSS:', 'VmHWM:')):
                            k, n, unit = line.split()
                            assert unit == 'kB'
                            metrics[k.rstrip(':') + '_bytes'] = int(n) * 1024
                    result = dict(calls=count, repeat=repeat, source_files=(count + 499) // 500, source_bytes=source_bytes, response_bytes=len(body), requests=requests, elapsed_seconds=round(time.monotonic() - start, 3), **metrics)
                    p.stdin.close()
                    exit_code = p.wait(timeout=10)
                    assert exit_code == 0, exit_code
                    result['exit_code'] = exit_code
                    results.append(result)
                    print(json.dumps(result), flush=True)
                finally:
                    if p.poll() is None:
                        p.kill()
                        p.wait()
                    p.stdout.close()
print(json.dumps(dict(binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), profile='caller-selected, not inferred', results=results), indent=2))
