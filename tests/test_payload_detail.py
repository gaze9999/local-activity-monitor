import json
import unittest

from local_activity_monitor.payload_detail import bounded_payload, parse_text


class PayloadDetailTests(unittest.TestCase):
    def test_yaml_subset_literals_and_unsupported_expressions(self):
        yaml = 'name: example\ncount: 0\nenabled: false\nitems:\n  - name: first\n    password: PRIVATE\n  - name: second\n    value: null\n'
        parsed = parse_text(yaml)
        self.assertEqual(parsed['format'], 'YAML subset')
        self.assertEqual(parsed['value']['count'], 0)
        self.assertFalse(parsed['value']['enabled'])
        self.assertEqual(parsed['value']['items'][1]['value'], None)
        self.assertNotIn('PRIVATE', json.dumps(bounded_payload(yaml)))
        self.assertEqual(parse_text('single: value', format='yaml')['value'], {'single': 'value'})
        self.assertEqual(parse_text(r'{name: "\uD83D\uDE00", letter: "\x41"}')['value'], {'name': '\U0001f600', 'letter': 'A'})
        for source, expected in [("{'ok': True, 'value': None, 'items': (0, False)}", {'ok': True, 'value': None, 'items': [0, False]}),
                                 ('{ok: true, value: null, items: [0, false], count: 1e2}', {'ok': True, 'value': None, 'items': [0, False], 'count': 100})]:
            self.assertEqual(parse_text(source)['value'], expected)
        for source in ('{value: run()}', '{value: variable}', '{value: `text ${secret}`}', "{'value': __import__('os').system('anything')}",
                       'name: example\nvalue: &anchor 1', 'name: example\nvalue: *anchor', 'name: example\nvalue: !!python/object thing',
                       'name: example\nvalue: |\n  line', 'name: example\nname: duplicate'):
            self.assertIsNone(parse_text(source), source)

    def test_common_formats_preserve_structure_and_mask_fields(self):
        for text, format in [('{"a": 1}\n{"b": 2}', 'JSONL'),
                             ('<result><password>PRIVATE</password><count>2</count></result>', 'XML'),
                             ('name,password\nexample,PRIVATE\n', 'CSV'),
                             ('name\tcount\nexample\t2\n', 'TSV')]:
            with self.subTest(format=format):
                self.assertEqual(parse_text(text)['format'], format)
                self.assertNotIn('PRIVATE', json.dumps(bounded_payload(text)['value']))
        self.assertIsNone(parse_text('<!DOCTYPE foo [<!ENTITY x SYSTEM "file:///private">]><foo>&x;</foo>'))

    def test_toml_dates_and_invalid_formats(self):
        import local_activity_monitor.payload_detail as details
        if details.tomllib:
            parsed=bounded_payload('updated = 2026-10-05T12:00:00Z\ncount = 0\n')['value']
            self.assertEqual(parsed['count'], 0)
            self.assertIn('2026-10-05', parsed['updated'])
        self.assertIsNone(parse_text('ordinary output\nnot structured'))
        self.assertEqual(bounded_payload('{invalid')['value'], '{invalid')
    def test_large_encoded_content_is_parsed_before_truncation(self):
        raw=json.dumps([{'type':'input_text','text':json.dumps({'status':'fulfilled','value':{'output':'line\n'*20000,'exit_code':0}})}])
        result=bounded_payload(raw)
        self.assertIsInstance(result['value'],list)
        self.assertIsInstance(result['value'][0]['text'],dict)
        self.assertEqual(result['value'][0]['text']['status'],'fulfilled')
        self.assertTrue(result['truncated'])
        self.assertLess(len(json.dumps(result)),70000)

    def test_embedded_credentials_and_invalid_numbers_are_masked(self):
        result=bounded_payload({'text':json.dumps({'authorization':'PRIVATE','items':[{'password':'PRIVATE'}]}),'value':float('inf')})
        self.assertNotIn('PRIVATE',json.dumps(result))
        self.assertIsNone(result['value']['value'])

    def test_plain_output_zero_and_null_are_preserved(self):
        self.assertEqual(bounded_payload('a\nb')['value'],'a\nb')
        self.assertEqual(bounded_payload({'exit_code':0,'output':None})['value'],{'exit_code':0,'output':None})


if __name__=='__main__':unittest.main()
