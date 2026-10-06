import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

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
        self.assertEqual(worker['skills'],['one'])
        self.assertEqual(worker['mcp_servers'],['private'])
        self.assertEqual(worker['manifest_source'],'.codex-plugin/plugin.json')

    def test_root_manifest_extensions_and_implicit_skill_directory(self):
        directory = self.home/'plugins/cache/provider/local/0.1.0'
        (directory/'skills/agent-governance').mkdir(parents=True)
        (directory/'skills/agent-governance/SKILL.md').write_text('PRIVATE_SKILL_CONTENT',encoding='utf-8')
        (directory/'plugin.json').write_text(json.dumps({'name':'local','version':'0.1.0','extensions':{'com.openai':{'interface':{'displayName':'Local plugin'}}}}),encoding='utf-8')
        item = read_plugins(self.home)['items'][0]
        self.assertEqual(item['manifest_source'],'plugin.json')
        self.assertEqual(item['display_name'],'Local plugin')
        self.assertEqual(item['skills'],['agent-governance'])
        self.assertEqual(item['skills_count'],1)
        self.assertEqual(item['mcp_count'],0)
        self.assertEqual(item['components'],['skills'])
        self.assertNotIn('PRIVATE_SKILL_CONTENT',json.dumps(item))

    def test_latest_directory_uses_manifest_version_and_mcp_names_only(self):
        directory = self.manifest('browser','latest',mcpServers='./.mcp.json',apps='./.app.json',hooks={'private':'SECRET'})
        (directory/'.codex-plugin/plugin.json').write_text(json.dumps({'version':'26.930.51102','mcpServers':'./.mcp.json','apps':'./.app.json','hooks':{'private':'SECRET'}}),encoding='utf-8')
        (directory/'.mcp.json').write_text(json.dumps({'mcpServers':{'browser':{'command':'SECRET_COMMAND','args':['SECRET'],'env':{'TOKEN':'SECRET'}}}}),encoding='utf-8')
        item = read_plugins(self.home)['items'][0]
        self.assertEqual(item['version'],'26.930.51102')
        self.assertEqual(item['mcp_servers'],['browser'])
        self.assertEqual(item['mcp_count'],1)
        self.assertEqual(item['components'],['mcpServers','apps','hooks'])
        self.assertEqual(item['mcp_status'],'available')
        self.assertNotIn('SECRET',json.dumps(item))

    def test_unsupported_paths_do_not_read_arbitrary_files(self):
        self.manifest('outside','1.0.0',skills='../private',mcpServers='../private.json')
        item = read_plugins(self.home)['items'][0]
        self.assertIsNone(item['skills_count'])
        self.assertIsNone(item['mcp_count'])
        self.assertEqual(item['skills_status'],'unsupported')
        self.assertEqual(item['mcp_status'],'unsupported')

    def test_unreadable_or_oversize_mcp_does_not_become_zero(self):
        directory = self.manifest('bad-mcp','1.0.0',mcpServers='./.mcp.json')
        item = read_plugins(self.home)['items'][0]
        self.assertIsNone(item['mcp_count'])
        self.assertEqual(item['mcp_status'],'missing')
        (directory/'.mcp.json').write_text('invalid',encoding='utf-8')
        self.assertEqual(read_plugins(self.home)['items'][0]['mcp_status'],'unavailable')
        (directory/'.mcp.json').write_text(' '*65537,encoding='utf-8')
        self.assertEqual(read_plugins(self.home)['items'][0]['mcp_status'],'truncated')

    def test_declared_missing_skills_stay_unknown(self):
        self.manifest('missing-skills','1.0.0',skills='./skills/')
        item = read_plugins(self.home)['items'][0]
        self.assertEqual(item['skills_status'],'missing')
        self.assertIsNone(item['skills'])
        self.assertIsNone(item['skills_count'])

    def test_total_manifest_byte_budget_is_preserved(self):
        self.manifest('first','1.0.0',description='x'*100)
        self.manifest('second','1.0.0',description='y'*100)
        report = {}
        with patch('local_activity_monitor.codex_plugins.READ_LIMIT',100):
            result = read_plugins(self.home,report)
        self.assertEqual(result['health'],'partial')
        self.assertEqual(result['items'][0]['metadata_status'],'truncated')
        self.assertIsNone(result['items'][0]['skills_count'])
        self.assertLessEqual(next(iter(report.values()))['bytes_read'],100)

    def test_directory_budget_marks_partial_counts(self):
        directory = self.manifest('bounded','1.0.0',skills='./skills/')
        for name in ('one','two','three'):
            (directory/'skills'/name).mkdir(parents=True)
            (directory/'skills'/name/'SKILL.md').write_text('PRIVATE',encoding='utf-8')
        with patch('local_activity_monitor.codex_plugins.DIRECTORY_LIMIT',5):
            report = {}
            result = read_plugins(self.home,report)
        self.assertEqual(result['health'],'partial')
        item = result['items'][0]
        self.assertEqual(item['skills_status'],'truncated')
        self.assertIsNone(item['skills_count'])
        self.assertLessEqual(next(iter(report.values()))['directories_read'],5)

    def test_manifest_symlink_is_not_read(self):
        directory = self.manifest('linked','1.0.0')
        target = self.home/'private.json'
        target.write_text('{"description":"SECRET"}',encoding='utf-8')
        path = directory/'.codex-plugin/plugin.json'
        path.unlink()
        try:
            path.symlink_to(target)
        except OSError:
            self.skipTest('symlinks unavailable')
        item = read_plugins(self.home)['items'][0]
        self.assertEqual(item['metadata_status'],'unavailable')
        self.assertNotIn('SECRET',json.dumps(item))

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
