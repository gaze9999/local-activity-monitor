"""Instruction precedence and registered workspace boundaries."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from local_activity_monitor.project_instructions import instructions, TOTAL_READ_LIMIT


class InstructionSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.parent = Path(self.temp.name).resolve()
        self.child = self.parent/'child'
        self.child.mkdir()
        (self.child/'AGENTS.md').write_text('Entry', encoding='utf-8')

    def registry(self):
        (self.parent/'.gitmodules').write_text('[submodule "child"]\npath = child\nurl = https://github.com/example/child.git\n', encoding='utf-8')
        (self.parent/'AGENTS.md').write_text('Parent', encoding='utf-8')
        source = self.parent/'project-agents/child/AGENTS.md'
        source.parent.mkdir(parents=True)
        source.write_text('Canonical', encoding='utf-8')
        self.profile = {'projects': [{'path': 'child', 'repository': 'https://github.com/example/child.git', 'instructions': 'project-agents/child/AGENTS.md'}]}
        self.save()
        return source

    def save(self):
        (self.parent/'workspace.json').write_text(json.dumps(self.profile), encoding='utf-8')

    def read(self):
        with patch('local_activity_monitor.project_instructions.subprocess.run', return_value=subprocess.CompletedProcess([], 0, 'https://github.com/example/child.git\n')):
            return instructions([self.child])

    def test_override_and_empty_override_precedence(self):
        override = self.child/'AGENTS.override.md'
        override.write_text('Override', encoding='utf-8')
        self.assertEqual(self.read()['documents'][0]['text'], 'Override')
        override.write_text('', encoding='utf-8')
        self.assertEqual(self.read()['documents'][0]['text'], 'Entry')

    def test_registered_parent_and_canonical_source_are_read(self):
        self.registry()
        self.assertEqual([d['text'] for d in self.read()['documents']], ['Parent', 'Canonical', 'Entry'])

    def test_markdown_link_cannot_expand_reads(self):
        (self.child/'AGENTS.md').write_text('Read [secret](../outside.md)', encoding='utf-8')
        (self.parent/'outside.md').write_text('PRIVATE_OUTSIDE', encoding='utf-8')
        self.assertNotIn('PRIVATE_OUTSIDE', json.dumps(self.read()))

    def test_registry_escape_and_wrong_identity_cannot_expand_reads(self):
        self.registry()
        for field, value in [('instructions', 'project-agents/../../outside.md'), ('repository', 'https://github.com/other/project.git'), ('repository', None)]:
            with self.subTest(field=field):
                original = self.profile['projects'][0][field]
                self.profile['projects'][0][field] = value
                self.save()
                self.assertEqual([d['text'] for d in self.read()['documents']], ['Entry'])
                self.profile['projects'][0][field] = original

    def test_git_identity_failure_retains_local_instructions(self):
        self.registry()
        with patch('local_activity_monitor.project_instructions.subprocess.run', return_value=subprocess.CompletedProcess([], 1, '')):
            self.assertEqual([d['text'] for d in instructions([self.child])['documents']], ['Entry'])

    def test_total_document_read_limit(self):
        roots = []
        for index in range(8):
            root = self.parent/str(index)
            root.mkdir()
            (root/'AGENTS.md').write_text('X'*100000, encoding='utf-8')
            roots.append(root)
        self.assertEqual(sum(d['read_bytes'] for d in instructions(roots)['documents']), TOTAL_READ_LIMIT)

    def test_playbook_override_keeps_explicit_public_source(self):
        self.registry()
        self.profile['projects'][0]['entry'] = 'AGENTS.override.md'
        self.save()
        (self.child/'AGENTS.override.md').write_text('Override', encoding='utf-8')
        self.assertEqual([d['text'] for d in self.read()['documents']], ['Parent', 'Canonical', 'Entry', 'Override'])


if __name__ == '__main__':
    unittest.main()
