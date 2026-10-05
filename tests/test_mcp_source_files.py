import json
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch

from local_activity_monitor.mcp_records import source_description
from local_activity_monitor.mcp_source_files import documents, read_document, write_document


class McpSourceFileTests(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root=Path(self.directory.name)
        if sys.platform=='darwin':
            self.root=self.root.resolve()
        (self.root/'src').mkdir()
        (self.root/'src/server.mjs').write_text('',encoding='utf-8')
        self.rules=self.root/'terms.yml'
        self.rules.write_text('version: 1\nrules: []\n',encoding='utf-8')
        (self.root/'.textlintrc.cjs').write_text("module.exports = {rulePaths: ['terms.yml']};",encoding='utf-8')
        (self.root/'package.json').write_text(json.dumps({'description':'A useful unknown MCP for editing documents.'}),encoding='utf-8')
        self.value={'command':'node','args':[str(self.root/'src/server.mjs'),str(self.root/'.textlintrc.cjs')]}

    def file(self,name):
        return next(item for item in documents(self.value).values() if item['name']==name)

    def test_unknown_source_uses_package_description_and_declared_rule_files(self):
        description=source_description(self.value,{})
        self.assertEqual(description['description_source'],'package.json')
        self.assertIn('useful unknown',description['description'])
        entry=self.file('terms.yml')
        self.assertTrue(entry['editable'])
        self.assertIn('modified_at',entry)
        self.assertFalse(self.file('package.json')['editable'])
        self.assertNotIn('textlintrc',json.dumps(description))

    def test_edit_is_atomic_and_rejects_a_concurrent_change(self):
        entry=read_document(self.value,self.file('terms.yml')['id'])
        saved=write_document(self.value,entry['id'],'version: 1\nrules: [hello]\n',entry['sha256'])
        self.assertEqual(saved['text'],'version: 1\nrules: [hello]\n')
        self.assertNotEqual(saved['sha256'],entry['sha256'])
        self.assertEqual(list(self.root.glob('.lam-*.tmp')),[])
        with self.assertRaises(FileExistsError):
            write_document(self.value,entry['id'],'overwrite',entry['sha256'])
        self.assertEqual(self.rules.read_text(encoding='utf-8'),saved['text'])

    def test_json_validation_and_credential_masking_prevent_data_loss(self):
        path=self.root/'settings.json'
        path.write_text('{"theme":"dark"}',encoding='utf-8')
        entry=read_document(self.value,self.file('settings.json')['id'])
        self.assertTrue(entry['editable'])
        for text in ('{invalid','{"value":NaN}','{"api_key":"PRIVATE"}'):
            with self.assertRaises(ValueError):
                write_document(self.value,entry['id'],text,entry['sha256'])
        self.assertEqual(path.read_text(encoding='utf-8'),'{"theme":"dark"}')
        path.write_text('{"api_key":"PRIVATE"}',encoding='utf-8')
        result=read_document(self.value,entry['id'])
        self.assertTrue(result['masked'])
        self.assertFalse(result['editable'])
        self.assertNotIn('PRIVATE',result['text'])

    def test_arbitrary_private_symlink_and_oversized_files_are_rejected(self):
        self.assertIsNone(read_document(self.value,'0'*32))
        (self.root/'auth.json').write_text('PRIVATE',encoding='utf-8')
        self.value['args'].append(str(self.root/'auth.json'))
        self.assertNotIn('auth.json',[entry['name'] for entry in documents(self.value).values()])
        entry=self.file('terms.yml')
        self.rules.write_bytes(b'x'*65537)
        result=read_document(self.value,entry['id'])
        self.assertTrue(result['truncated'])
        self.assertFalse(result['editable'])
        with patch.object(Path,'is_symlink',return_value=True),patch.object(Path,'open',side_effect=AssertionError('symlink read')):
            self.assertEqual(documents(self.value),{})

    def test_description_cache_is_bounded_and_readme_navigation_is_skipped(self):
        (self.root/'package.json').write_text('{}',encoding='utf-8')
        (self.root/'README.md').write_text('# Header\n\nWebsite · Documentation · GitHub\n\nExtract and check local documents using the configured MCP.\n\n'+('x'*70000),encoding='utf-8')
        cache={}
        result=source_description(self.value,cache)
        self.assertEqual(result['description_source'],'README.md')
        self.assertTrue(result['description'].startswith('Extract and check'))
        with patch.object(Path,'open',side_effect=AssertionError('cache reread')):
            self.assertEqual(source_description(self.value,cache),result)


if __name__=='__main__':unittest.main()
