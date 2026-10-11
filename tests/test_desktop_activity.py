import json
import unittest
from local_activity_monitor.desktop_activity import desktop_activity
from local_activity_monitor.device_info import system_fonts
from local_activity_monitor.operation_records import workflow_operations
from unittest.mock import Mock, patch


class DesktopActivityTests(unittest.TestCase):
    def test_remote_dot_metadata_is_bounded_and_excludes_private_state(self):
        identity='00000000-0000-4000-8000-000000000001'
        host='remote-control:demo'
        state={'electron-persisted-atom-state':{
            'remote-thread-summaries-v3:'+host:[{'conversationId':identity,'title':'Demo','updatedAt':1800000000,'threadSource':'dot','threadRuntimeStatus':{'type':'completed'},'privatePath':'PRIVATE_PATH','message':'PRIVATE_MESSAGE'}],
            'device-enrollment':{'secret':'PRIVATE_KEY'},
            'orbit-activity-snapshots-v1':[{'data':[{'threadId':identity,'hostId':host,'timestampMs':1800000000000}]}]}}
        entries,dots={},{'events':[]}
        desktop_activity(state,entries,dots)
        self.assertEqual(entries[identity]['trigger'],'dot')
        self.assertEqual(dots['computers'][0]['thread_count'],1)
        self.assertEqual(dots['computers'][0]['connection_status'],'unknown')
        self.assertEqual(len(dots['activity']),2)
        self.assertNotIn('PRIVATE',json.dumps([entries,dots]))

    def test_manual_dots_query_reads_only_bounded_desktop_projection(self):
        from pathlib import Path
        from tempfile import TemporaryDirectory
        from local_activity_monitor.desktop_activity import read_dots
        with TemporaryDirectory() as folder:
            root=Path(folder)
            self.assertEqual(read_dots(root)['health'],'error')
            path=root/'.codex-global-state.json'
            identity='00000000-0000-4000-8000-000000000001'
            path.write_text(json.dumps({'electron-persisted-atom-state':{'orbit-outputs-v1':[{'threadId':identity,'artifact':{'type':'text','producedAtMs':0,'content':'PRIVATE_CONTENT'},'accountId':'PRIVATE_ID'}],'credential':'PRIVATE_KEY'}}),encoding='utf8')
            value=read_dots(root)
            self.assertEqual(value['health'],'ok')
            self.assertEqual(value['events'][0]['timestamp'],'1970-01-01T00:00:00.000Z')
            self.assertNotIn('PRIVATE',json.dumps(value))
            self.assertNotIn('_retained_events',value)
            path.write_text('[]',encoding='utf8')
            self.assertEqual(read_dots(root)['health'],'error')
            path.write_text('{}',encoding='utf8')
            with patch('local_activity_monitor.codex_metadata.APP_STATE_LIMIT',1):
                self.assertEqual(read_dots(root)['health'],'error')

    def test_fonts_are_local_names_and_invalid_names_are_omitted(self):
        with patch('local_activity_monitor.device_info.sys.platform','darwin'),patch('local_activity_monitor.device_info._native_fonts',return_value={'Menlo','PingFang TC','bad;url(x)','/private/font.ttf',''}):
            self.assertEqual(system_fonts(),['Menlo','PingFang TC'])
        with patch('local_activity_monitor.device_info.sys.platform','linux'),patch('local_activity_monitor.device_info.shutil.which',return_value='/usr/bin/fc-list'),patch('local_activity_monitor.device_info.subprocess.run',return_value=Mock(returncode=0,stdout='Family A,Family B\nFamily A\n')):
            self.assertEqual(system_fonts(),['Family A','Family B'])

    def test_recent_dot_computer_is_visible_without_a_catalog_thread(self):
        identity='00000000-0000-4000-8000-000000000001'
        state={'electron-persisted-atom-state':{'orbit-activity-snapshots-v1':[{'data':[
            {'threadId':identity,'hostId':'remote-control:dot-only','timestampMs':1800000000000}]}]}}
        entries,dots={},{'events':[]}
        desktop_activity(state,entries,dots)
        self.assertEqual(len(dots['computers']),1)
        self.assertEqual(dots['computers'][0],{'host_id':'remote-control:dot-only','last_activity_at':'2027-01-15T08:00:00.000Z','thread_count':0,'connection_status':'unknown'})
        self.assertEqual(dots['activity'][0]['thread_id'],identity)

    def test_cloud_dot_activity_is_kept_without_creating_a_computer(self):
        identity='00000000-0000-4000-8000-000000000001'
        state={'electron-persisted-atom-state':{'orbit-activity-snapshots-v1':[{'data':[
            {'threadId':identity,'hostId':'durable','timestampMs':1800000000000,'message':'PRIVATE'},
            {'threadId':identity,'hostId':'chatgpt:demo','timestampMs':1800000000000},
            {'threadId':identity,'hostId':'https://example.test','timestampMs':1800000000000}]}]}}
        entries={identity:{'environment':'cloud','trigger':'dot','updated_at':'2027-01-15T08:00:00.000Z'}}
        dots={'events':[]}
        desktop_activity(state,entries,dots)
        self.assertEqual(len(dots['activity']),3)
        self.assertTrue(all(item['environment']=='cloud' for item in dots['activity']))
        self.assertEqual(dots['computers'],[])
        self.assertNotIn('PRIVATE',json.dumps(dots))

    def test_scheduled_cloud_activity_keeps_its_source_and_recent_order(self):
        from local_activity_monitor.codex_schedules import read_schedules
        from pathlib import Path
        metadata={
            'cloud':{'environment':'cloud','trigger':'schedule','updated_at':'2026-10-11T12:00:00Z'},
            'remote':{'environment':'remote','trigger':'heartbeat','updated_at':'2026-10-11T13:00:00Z'},
            'dot':{'environment':'cloud','trigger':'dot','updated_at':'2026-10-11T15:00:00Z'},
            'user':{'environment':'cloud','trigger':'user','updated_at':'2026-10-11T14:00:00Z'}}
        value=read_schedules(Path('tests/__missing_cloud_home__').resolve(),metadata=metadata)
        self.assertEqual([item['thread_id'] for item in value['remote_activities']],['remote','cloud'])
        self.assertEqual(value['remote_activities'][1]['environment'],'cloud')
        self.assertNotIn('next_run_at',value['remote_activities'][1])

    def test_manual_dots_query_respects_the_activity_window(self):
        from datetime import datetime, timedelta, timezone
        from pathlib import Path
        import tempfile
        from local_activity_monitor.desktop_activity import read_dots
        now = datetime.now(timezone.utc)
        identity = '00000000-0000-4000-8000-000000000001'
        state = {'electron-persisted-atom-state':{'orbit-activity-snapshots-v1':[{'data':[
            {'threadId':identity, 'hostId':'durable', 'timestampMs':int((now-timedelta(minutes=10)).timestamp()*1000)},
            {'threadId':identity, 'hostId':'durable', 'timestampMs':int((now-timedelta(days=2)).timestamp()*1000)}]}]}}
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            (home/'.codex-global-state.json').write_text(json.dumps(state), encoding='utf-8')
            self.assertEqual(len(read_dots(home, 'all')['activity']), 2)
            recent = read_dots(home, '24h')
            self.assertEqual((recent['health'], recent['window'], len(recent['activity'])), ('ok', '24h', 1))

    def test_windows_absolute_python_validation_commands_are_recognized(self):
        for command in (r"& 'C:\runtime\python.exe' -X utf8 -I -B -m unittest tests.test_demo",'python -m pytest','node --check app.js'):
            with self.subTest(command=command):
                _,checks=workflow_operations({'type':'function_call','name':'functions.exec_command','arguments':json.dumps({'cmd':command})})
                self.assertEqual(len(checks),1)
        _,checks=workflow_operations({'type':'function_call','name':'exec_command','arguments':json.dumps({'cmd':'Write-Output "python -m unittest"'})})
        self.assertEqual(checks,[])
