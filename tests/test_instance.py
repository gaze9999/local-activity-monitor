import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch

from local_activity_monitor.server import existing_instance, home_id, main


class InstanceTests(unittest.TestCase):
    def test_probe_requires_application_and_same_codex_home(self):
        home=Path('/test/home')
        for value, expected in [({'application':'local-activity-monitor','home_id':home_id(home)},'http://127.0.0.1:8791/'),({'application':'other','home_id':home_id(home)},None),({'application':'local-activity-monitor','home_id':'other'},None),([],None)]:
            with patch('local_activity_monitor.server.build_opener') as opener:
                opener.return_value.open.return_value.__enter__.return_value=io.BytesIO(json.dumps(value).encode())
                self.assertEqual(existing_instance(8791,home),expected)

    def test_existing_monitor_is_reused_without_starting_collectors(self):
        with patch('local_activity_monitor.server.existing_instance',return_value='http://127.0.0.1:8791/'),patch('local_activity_monitor.server.Dashboard') as dashboard,patch('local_activity_monitor.server.ThreadingHTTPServer') as server,patch('local_activity_monitor.server.webbrowser.open') as browser:
            self.assertEqual(main(['--port','8791','--open']),0)
            dashboard.assert_not_called();server.assert_not_called()
            browser.assert_called_once_with('http://127.0.0.1:8791/')

    def test_unrecognized_busy_port_is_reported_without_collector_start(self):
        with patch('local_activity_monitor.server.existing_instance',return_value=None),patch('local_activity_monitor.server.ThreadingHTTPServer',side_effect=OSError()),patch('local_activity_monitor.server.Dashboard') as dashboard:
            self.assertEqual(main(['--port','8791']),1)
            dashboard.assert_not_called()


if __name__=='__main__':unittest.main()
