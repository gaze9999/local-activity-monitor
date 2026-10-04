import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.collectors import CodexCollector
from local_activity_monitor.monitor_state import MonitorState
from local_activity_monitor.server import Dashboard, configure


class MonitorStateTests(unittest.TestCase):
    def test_default_settings_are_stable_and_restore_customizations(self):
        with tempfile.TemporaryDirectory() as directory:
            dashboard=Dashboard(Path(directory),codex=True,max_files=100)
            defaults=dashboard.snapshot('24h')['default_settings']
            dashboard.set_settings({'interval':3,'max_files':50,'track_all':True,'observations':{'codex':False},'mcp_descriptions':{'future_server':'Custom description'}})
            snapshot=dashboard.snapshot('24h')
            self.assertEqual(snapshot['default_settings'],defaults)
            snapshot['default_settings']['observations']['codex']=False
            self.assertTrue(dashboard.snapshot('24h')['default_settings']['observations']['codex'])
            dashboard.set_settings({**defaults,'replace_customizations':True})
            self.assertEqual(dashboard.settings(),defaults)

    def test_program_log_survives_restart_and_rotates_with_fixed_size(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'monitor.jsonl'
            with patch.object(MonitorState,'JOURNAL_LIMIT',1024):
                state=MonitorState(path)
                state.failed(OSError('PRIVATE_SECRET'))
                restarted=MonitorState(path)
                self.assertTrue(any(e['kind']=='refresh_failed' for e in restarted.logs))
                self.assertEqual(len(restarted.events),1)
                for _ in range(100):restarted.event('settings_applied')
                self.assertLessEqual(path.stat().st_size,1024)
                self.assertLessEqual(path.with_name(path.name+'.1').stat().st_size,1024)
                self.assertEqual(len(list(Path(directory).iterdir())),2)
                self.assertNotIn('PRIVATE',path.read_text())

    def test_program_log_write_failure_does_not_stop_metrics(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'monitor.jsonl';state=MonitorState(path)
            with patch.object(Path,'open',side_effect=PermissionError('PRIVATE_PATH')):
                state.failed(ValueError('PRIVATE_EXCEPTION'))
            state.refreshed({'refresh_ms':1,'cpu_ms':1,'read_bytes':0})
            self.assertEqual(state.snapshot()['health'],'ok')
            self.assertEqual(state.log_snapshot()['health'],'ok')
            self.assertTrue(any(e['kind']=='refresh_failed' for e in state.logs))
            self.assertNotIn('PRIVATE',json.dumps(state.log_snapshot()))

    def test_history_events_and_error_text_are_bounded(self):
        state=MonitorState()
        for index in range(500):
            state.refreshed({'refresh_ms':index,'cpu_ms':1,'read_bytes':0})
            state.event('settings_applied')
        state.failed(OSError('PRIVATE_PATH PRIVATE_CREDENTIAL'))
        state.failed(OSError('PRIVATE_OTHER'))
        snapshot=state.snapshot()
        self.assertEqual(len(snapshot['history']),360)
        self.assertEqual(len(snapshot['events']),200)
        self.assertEqual(snapshot['history'][0]['refresh_ms'],140)
        self.assertEqual(snapshot['errors'],2)
        self.assertEqual(snapshot['error_type'],'OSError')
        self.assertNotIn('PRIVATE',json.dumps(snapshot))
        state.refreshed({'refresh_ms':1,'cpu_ms':1,'read_bytes':0})
        self.assertEqual(state.snapshot()['events'][-1]['kind'],'recovered')

    def test_failed_refresh_keeps_last_data_and_recovers(self):
        with tempfile.TemporaryDirectory() as folder:
            dashboard=Dashboard(Path(folder),codex=True);dashboard.refresh()
            original=dashboard.snapshot('24h')
            with patch.object(dashboard.codex,'refresh',side_effect=OSError('PRIVATE')):
                with self.assertRaises(OSError):dashboard.refresh()
            failed=dashboard.snapshot('24h')
            self.assertEqual(failed['codex'],original['codex'])
            self.assertEqual(failed['updated_at'],original['updated_at'])
            self.assertEqual(failed['monitor']['health'],'error')
            dashboard.refresh();self.assertEqual(dashboard.snapshot('24h')['monitor']['health'],'ok')

    def test_first_refresh_failure_keeps_renderable_snapshot(self):
        with tempfile.TemporaryDirectory() as folder:
            dashboard=Dashboard(Path(folder),codex=True)
            with patch.object(dashboard.codex,'refresh',side_effect=OSError('PRIVATE')):
                with self.assertRaises(OSError):dashboard.refresh()
            value=dashboard.snapshot('24h')
            self.assertIsNone(value['updated_at'])
            self.assertEqual(value['monitor']['health'],'error')
            self.assertEqual(value['codex']['threads'],[])
            self.assertEqual(value['mcp']['events'],[])
            self.assertIn('observations',value['settings'])
            self.assertNotIn('PRIVATE',json.dumps(value))

    def test_disabled_recording_import_does_not_create_absent_source(self):
        with tempfile.TemporaryDirectory() as folder:
            home=Path(folder);dashboard=Dashboard(home)
            dashboard.set_settings({'recording':False,'replace_customizations':True})
            self.assertFalse((home/'monitoring/jev-monitor.json').exists())
            self.assertFalse(dashboard.snapshot('24h')['jev']['enabled'])

    def test_configuration_replacement_and_validation_precede_recording(self):
        with tempfile.TemporaryDirectory() as folder:
            home=Path(folder);dashboard=Dashboard(home)
            configure(home,False,home/'state/isolated.sqlite3')
            before=(home/'monitoring/jev-monitor.json').read_bytes()
            for value in ({'recording':True,'interval':0},{'recording':True,'replace_customizations':1},{'recording':'true'}):
                with self.assertRaises(ValueError):dashboard.set_settings(value)
            self.assertEqual((home/'monitoring/jev-monitor.json').read_bytes(),before)
            dashboard.set_settings({'mcp_sources':{'old':False},'tool_descriptions':{'old_tool':'old'}})
            dashboard.set_settings({'replace_customizations':True,'mcp_sources':{},'mcp_categories':{},'tool_descriptions':{'future_tool':'future'},'recording':False})
            self.assertEqual(dashboard.mcp_sources,{})
            self.assertEqual(dashboard.tool_descriptions,{'future_tool':'future'})

    def test_total_call_retention_preserves_latest_records(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);collector=CodexCollector(root);collector.CALL_LIMIT=3
            for index in range(2):
                records=[{'type':'response_item','timestamp':f'2026-10-03T00:00:0{index*2+offset}Z','payload':{'type':'function_call','name':'future_tool','call_id':f'call-{index}-{offset}','arguments':'{}'}} for offset in range(2)]
                (root/f'rollout-{index}.jsonl').write_text('\n'.join(map(json.dumps,records))+'\n')
            collector.refresh()
            calls={key for state in collector.files.values() for key in state['calls']}
            self.assertEqual(calls,{'call-0-1','call-1-0','call-1-1'})
            self.assertEqual(collector.trimmed_calls,1)

    def test_track_all_keeps_file_cap(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            for index in range(4):(root/f'rollout-{index}.jsonl').write_text('')
            collector=CodexCollector(root);collector.FILE_LIMIT=3;collector.track_all=True;collector.refresh()
            self.assertEqual(len(collector.files),3)

    def test_http_counts_and_sizes_do_not_store_routes(self):
        state=MonitorState();state.requested(200,1000,120);state.requested(403)
        value=state.snapshot()
        self.assertEqual((value['requests'],value['http_errors'],value['snapshot_bytes'],value['transfer_bytes']),(2,1,1000,120))


if __name__=='__main__':unittest.main()
