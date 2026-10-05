import json
from pathlib import Path
import tempfile
import unittest

from local_activity_monitor.codex_plugins import read_plugins
from local_activity_monitor.collectors import CodexCollector


class PluginSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name).resolve()

    def manifest(self, name, version, **fields):
        directory = self.home/'plugins/cache/provider'/name/version
        (directory/'.codex-plugin').mkdir(parents=True)
        (directory/'.codex-plugin/plugin.json').write_text(json.dumps({'name':name,'version':version,**fields}),encoding='utf-8')
        return directory

    def test_latest_cache_and_explicit_configuration_are_distinct_without_runtime_arguments(self):
        self.manifest('worker','1.9.0')
        directory = self.manifest('worker','1.10.0',skills='./skills/',mcpServers={'private':{'command':'SECRET_COMMAND','env':{'API_KEY':'SECRET'}}},interface={'displayName':'Worker'})
        (directory/'skills/one').mkdir(parents=True)
        (directory/'skills/one/SKILL.md').write_text('PRIVATE_SKILL_CONTENT',encoding='utf-8')
        self.manifest('cached','2.0.0')
        self.home.joinpath('config.toml').write_text('[plugins."worker@provider"]\nenabled=false\n[plugins."missing@provider"]\nenabled=true\n[mcp_servers.secret]\nenv={API_KEY="SECRET"}\n',encoding='utf-8')
        report = {}
        result = read_plugins(self.home,report)
        worker = next(item for item in result['items'] if item['name']=='worker')
        self.assertEqual((worker['version'],worker['enabled'],worker['skills_count'],worker['mcp_count']),('1.10.0',False,1,1))
        cached = next(item for item in result['items'] if item['name']=='cached')
        self.assertEqual(cached['state'],'cached')
        self.assertIsNone(cached['enabled'])
        missing = next(item for item in result['items'] if item['name']=='missing')
        self.assertIsNone(missing['version'])
        self.assertTrue(missing['enabled'])
        self.assertNotIn('SECRET',json.dumps([result,report]))
        self.assertNotIn('PRIVATE_SKILL_CONTENT',json.dumps([result,report]))

    def test_missing_bad_manifest_and_disabled_collection(self):
        self.assertEqual(read_plugins(self.home)['health'],'missing')
        directory = self.manifest('broken','1.0.0')
        (directory/'.codex-plugin/plugin.json').write_text('invalid',encoding='utf-8')
        self.assertEqual(read_plugins(self.home)['health'],'partly_unavailable')
        root = self.home/'sessions'
        root.mkdir()
        collector = CodexCollector(root)
        collector.features['metadata'] = False
        self.assertEqual(collector.snapshot()['plugins'],{'items':[],'health':'disabled'})
