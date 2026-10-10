"""Compare warmed, isolated Windows source monitoring and process I/O."""
import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time


ROOT = Path(__file__).resolve().parents[1]
IO_FIELDS = ('read_operations', 'write_operations', 'other_operations', 'read_bytes', 'write_bytes', 'other_bytes')


class IoCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in IO_FIELDS]


class MemoryCounters(ctypes.Structure):
    _fields_ = [('cb', ctypes.c_uint32), ('faults', ctypes.c_uint32)]+[(name, ctypes.c_size_t) for name in
        ('peak', 'working', 'peak_paged', 'paged', 'peak_nonpaged', 'nonpaged', 'pagefile', 'peak_pagefile')]


def native_metrics():
    kernel = ctypes.WinDLL('kernel32.dll', use_last_error=True, winmode=0x800)
    kernel.GetCurrentProcess.argtypes, kernel.GetCurrentProcess.restype = [], ctypes.c_void_p
    kernel.GetProcessIoCounters.argtypes = [ctypes.c_void_p, ctypes.POINTER(IoCounters)]
    kernel.GetProcessIoCounters.restype = ctypes.c_int
    psapi = ctypes.WinDLL('psapi.dll', use_last_error=True, winmode=0x800)
    psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.POINTER(MemoryCounters), ctypes.c_uint32]
    psapi.GetProcessMemoryInfo.restype = ctypes.c_int
    process = kernel.GetCurrentProcess()
    def sample():
        io, memory = IoCounters(), MemoryCounters()
        memory.cb = ctypes.sizeof(memory)
        if not kernel.GetProcessIoCounters(process, ctypes.byref(io)) or not psapi.GetProcessMemoryInfo(process, ctypes.byref(memory), memory.cb):
            raise ctypes.WinError(ctypes.get_last_error())
        return {**{name: getattr(io, name) for name in IO_FIELDS}, 'rss_bytes': memory.working}
    return sample


def calibration():
    sample = native_metrics()
    with tempfile.TemporaryDirectory(prefix='io-calibration-', dir=ROOT/'.local') as folder:
        path = Path(folder)/'numeric-fixture.bin'
        payload = b'x'*(256*1024)
        before = sample()
        path.write_bytes(payload)
        assert path.read_bytes() == payload
        after = sample()
    result = {name: after[name]-before[name] for name in IO_FIELDS}
    assert result['read_bytes'] >= len(payload) and result['write_bytes'] >= len(payload), result
    return {'payload_bytes': len(payload), **result}


def freeze_sources(target, ref=None, source=None):
    destination = target/'src/local_activity_monitor'
    destination.mkdir(parents=True)
    if ref:
        paths = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', ref, 'src/local_activity_monitor'], cwd=ROOT, text=True).splitlines()
        for relative in paths:
            if relative.endswith('.py'):
                (destination/Path(relative).name).write_bytes(subprocess.check_output(['git', 'show', ref+':'+relative], cwd=ROOT))
    else:
        for path in ((source or ROOT)/'src/local_activity_monitor').glob('*.py'):
            (destination/path.name).write_bytes(path.read_bytes())
    return hashlib.sha256(b''.join(path.read_bytes() for path in sorted(destination.glob('*.py')))).hexdigest()


def fixture_writer(path, start, count):
    target = Path(path).resolve()
    if not target.is_relative_to((ROOT/'.local').resolve()) or target.name != 'rollout-000.jsonl':
        raise ValueError('Writer must target an isolated fixture inside .local')
    for index in range(count):
        time.sleep(max(0, start+index+1-time.monotonic()))
        event = {'type': 'event_msg', 'timestamp': datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z'),
            'payload': {'type': 'token_count', 'info': {'total_token_usage': {'input_tokens': index+1, 'output_tokens': 1, 'total_tokens': index+2}}}}
        with target.open('a', encoding='utf-8') as stream:
            stream.write(json.dumps(event)+'\n')


def worker(source, mode, duration, case):
    sys.path.insert(0, str(Path(source)/'src'))
    from local_activity_monitor.server import Dashboard
    sample = native_metrics()
    with tempfile.TemporaryDirectory(prefix='source-io-', dir=ROOT/'.local') as folder:
        home = Path(folder)
        sessions = home/'sessions'
        sessions.mkdir()
        stamp = datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z')
        for index in range(12):
            events = [{'type': 'session_meta', 'timestamp': stamp, 'payload': {'id': f'00000000-0000-4000-8000-{index+1:012d}'}},
                      {'type': 'turn_context', 'timestamp': stamp, 'payload': {'model': 'fixture-model'}}]
            (sessions/f'rollout-{index:03}.jsonl').write_text(''.join(json.dumps(event)+'\n' for event in events), encoding='utf-8')
        app = Dashboard(home, codex=True, interval=10)
        app.monitor.load_device = lambda: None
        app.web_revision = lambda: 'fixture-assets'
        app.observations['worktrees'] = False
        initial_before = sample()
        initial_cpu, initial_start = time.process_time(), time.perf_counter()
        app.refresh()
        initial_after = sample()
        initial = {'elapsed_ms': round((time.perf_counter()-initial_start)*1000, 2),
            'cpu_ms': round((time.process_time()-initial_cpu)*1000, 2), 'session_read_bytes': app.codex.read_bytes,
            **{name: initial_after[name]-initial_before[name] for name in IO_FIELDS}}
        assert len(app.cache['all']['codex']['threads']) == 12
        calls, warm = [], threading.Event()
        collect = app.poll_once
        def measured():
            result = collect()
            calls.append(time.perf_counter())
            warm.set()
            return result
        app.poll_once = measured
        app.thread.start()
        writer = None
        try:
            if not warm.wait(15):
                raise TimeoutError('Initial worker collection did not complete')
            # Drain startup notifications before measuring steady idle behavior.
            time.sleep(.5)
            if mode == 'old-paused':
                with app.refresh_lock:
                    app.activity.last_change = time.monotonic()-301
                    app.activity.check(app.codex, 5, app.observations['metadata'])
                    app.cache_activity()
                    assert app.activity.paused, 'Fixture must reach the old idle gate'
            source_writes = max(1, int(duration)-15) if case == 'active' else 0
            target = sessions/'rollout-000.jsonl'
            original_size = target.stat().st_size
            if source_writes:
                begin = time.monotonic()+1
                writer = subprocess.Popen([sys.executable, '-X', 'utf8', '-I', '-B', str(Path(__file__).resolve()),
                    '--fixture-writer', str(target), str(begin), str(source_writes)],
                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
                time.sleep(max(0, begin-time.monotonic()))
            before = sample()
            before_cpu, start = time.process_time(), time.perf_counter()
            before_calls, before_refreshes = len(calls), app.monitor.refreshes
            before_read = app.codex.read_bytes
            if app.stop.wait(duration):
                raise RuntimeError('Collector stopped before the measurement ended')
            elapsed, cpu = time.perf_counter()-start, time.process_time()-before_cpu
            after = sample()
            result = {'mode': mode, 'case': case, 'seconds': round(elapsed, 3), 'cpu_ms': round(cpu*1000, 2),
                'initial_collection': initial,
                'single_core_percent': round(cpu/elapsed*100, 5), 'checks': len(calls)-before_calls,
                'collections': app.monitor.refreshes-before_refreshes, 'session_read_bytes': app.codex.read_bytes-before_read,
                'rss_start_bytes': before['rss_bytes'], 'rss_end_bytes': after['rss_bytes'],
                'paused': app.activity_status().get('paused'), 'health': app.monitor.health,
                **{name: after[name]-before[name] for name in IO_FIELDS}}
            assert result['health'] == 'ok', result
            if mode == 'new' and case == 'idle':
                assert result['collections'] == 0 and result['checks'] == 0, result
            if mode == 'old-paused':
                assert result['paused'] and result['collections'] == 0, result
            if writer:
                if writer.wait(1) != 0:
                    raise RuntimeError('Fixture writer failed')
                thread = next(thread for thread in app.cache['all']['codex']['threads'] if thread['thread_id'].endswith('000000000001'))
                result.update(source_writes=source_writes, fixture_written_bytes=target.stat().st_size-original_size,
                    latest_input_tokens=thread.get('tokens', {}).get('input_tokens'))
                assert result['latest_input_tokens'] == source_writes, result
        finally:
            if writer and writer.poll() is None:
                writer.terminate()
                writer.wait(5)
            app.stop.set()
            if hasattr(app, 'source_changed'):
                app.source_changed.set()
            app.thread.join(5)
            if app.thread.is_alive():
                raise RuntimeError('Benchmark worker did not stop')
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', default='a1cc7fe')
    parser.add_argument('--baseline-source', type=Path, help='Frozen source checkout inside .local, compared instead of a Git commit')
    parser.add_argument('--duration', type=float, default=65)
    parser.add_argument('--runs', type=int, default=2)
    parser.add_argument('--case', choices=('idle', 'active'), default='idle')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--worker', nargs=2, metavar=('SOURCE', 'MODE'), help=argparse.SUPPRESS)
    parser.add_argument('--fixture-writer', nargs=3, metavar=('PATH', 'START', 'COUNT'), help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.fixture_writer:
        fixture_writer(args.fixture_writer[0], float(args.fixture_writer[1]), int(args.fixture_writer[2]))
        return
    if os.name != 'nt':
        parser.error('This comparison uses Windows process I/O counters')
    if not 1 <= args.duration <= 600 or not 1 <= args.runs <= 10:
        parser.error('duration must be 1-600 seconds and runs must be 1-10')
    if args.case == 'active' and args.duration < 20:
        parser.error('Active comparison needs at least 20 seconds including the drain period')
    if args.worker:
        print(json.dumps(worker(*args.worker, args.duration, args.case)))
        return
    output = (args.output or ROOT/('.local/source-update-io-performance.json' if args.case == 'idle' else '.local/source-update-io-active-performance.json')).resolve()
    staging = ROOT/'.local'
    staging.mkdir(exist_ok=True)
    if not output.is_relative_to(staging.resolve()):
        parser.error('Output must stay inside this checkout .local directory')
    baseline = None
    if args.baseline_source:
        args.baseline_source = args.baseline_source.resolve()
        if not args.baseline_source.is_relative_to(staging.resolve()) or not (args.baseline_source/'src/local_activity_monitor/server.py').is_file():
            parser.error('Baseline source must be a frozen source checkout inside .local')
    else:
        baseline = subprocess.check_output(['git', 'rev-parse', '--verify', args.baseline+'^{commit}'], cwd=ROOT, text=True).strip()
    evidence = {'timestamp': datetime.now(timezone.utc).isoformat(timespec='seconds'), 'python': sys.version.split()[0],
        'baseline_commit': baseline, 'fixture_files': 12, 'debug_enabled': False, 'http_enabled': False,
        'duration': args.duration, 'runs': args.runs, 'case': args.case, 'calibration': calibration(), 'samples': []}
    with tempfile.TemporaryDirectory(prefix='source-comparison-', dir=staging) as folder:
        frozen = Path(folder)
        evidence['baseline_source_sha256'] = freeze_sources(frozen/'old', baseline, args.baseline_source)
        evidence['candidate_source_sha256'] = freeze_sources(frozen/'new')
        for run in range(args.runs):
            for mode in (('old', 'new') if run%2 == 0 else ('new', 'old')):
                source = frozen/mode
                raw = subprocess.check_output([sys.executable, '-X', 'utf8', '-I', '-B', str(Path(__file__).resolve()),
                    '--worker', str(source), mode, '--duration', str(args.duration), '--case', args.case], cwd=ROOT, text=True)
                result = json.loads(raw)|{'run': run+1}
                evidence['samples'].append(result)
                print(json.dumps(result), flush=True)
                output.write_text(json.dumps(evidence, indent=2)+'\n', encoding='utf-8')
        if args.case == 'idle' and not args.baseline_source:
            raw = subprocess.check_output([sys.executable, '-X', 'utf8', '-I', '-B', str(Path(__file__).resolve()),
                '--worker', str(frozen/'old'), 'old-paused', '--duration', str(args.duration)], cwd=ROOT, text=True)
            evidence['samples'].append(json.loads(raw)|{'run': 1, 'idle_gate': 'fixture last_change set 301 seconds earlier'})
    output.write_text(json.dumps(evidence, indent=2)+'\n', encoding='utf-8')
    if args.case == 'idle' and not args.baseline_source:
        print(json.dumps(evidence['samples'][-1]), flush=True)


if __name__ == '__main__':
    main()
