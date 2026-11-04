"""Byte-level fixture stability across roots and native newline defaults."""
from datetime import datetime, timezone
import json
from pathlib import Path
import tempfile
import unittest

from contract_fixture import RECORD_BYTES, SessionWriter, append_activity, build


class ContractFixtureTests(unittest.TestCase):
    def test_offsets_survive_root_length_and_unicode(self):
        anchor = datetime(2026, 1, 1, tzinfo=timezone.utc)
        with tempfile.TemporaryDirectory() as directory:
            roots = [Path(directory)/'short', Path(directory)/'較長的 fixture root with spaces']
            fixtures = [build(root, anchor) for root in roots]
            for appended in (False, True):
                if appended:
                    for fixture in fixtures:
                        append_activity(fixture, anchor)
                layouts = []
                for fixture in fixtures:
                    self.assertEqual((fixture['home']/'config.toml').stat().st_size, RECORD_BYTES)
                    layout = {}
                    for path in sorted(fixture['sessions'].glob('*.jsonl')):
                        data = path.read_bytes()
                        self.assertNotIn(b'\r', data)
                        lines = data.splitlines(keepends=True)
                        for line in lines:
                            try:
                                json.loads(line)
                            except json.JSONDecodeError:
                                continue
                            self.assertEqual(len(line), RECORD_BYTES)
                        layout[path.name] = [len(line) for line in lines]
                    layouts.append(layout)
                self.assertEqual(*layouts)

    def test_oversize_record_is_not_truncated(self):
        writer = SessionWriter(Path('unused'), datetime(2026, 1, 1, tzinfo=timezone.utc))
        writer.add('event_msg', {'text': 'x'*RECORD_BYTES}, 0)
        with self.assertRaisesRegex(ValueError, 'byte budget'):
            writer.serialized()
