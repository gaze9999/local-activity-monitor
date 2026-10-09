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

    def test_windows_absolute_python_validation_commands_are_recognized(self):
        for command in (r"& 'C:\runtime\python.exe' -X utf8 -I -B -m unittest tests.test_demo",'python -m pytest','node --check app.js'):
            with self.subTest(command=command):
                _,checks=workflow_operations({'type':'function_call','name':'functions.exec_command','arguments':json.dumps({'cmd':command})})
                self.assertEqual(len(checks),1)
        _,checks=workflow_operations({'type':'function_call','name':'exec_command','arguments':json.dumps({'cmd':'Write-Output "python -m unittest"'})})
        self.assertEqual(checks,[])
