import json
import unittest
from concurrent.futures import ThreadPoolExecutor
from contextvars import copy_context

from local_activity_monitor.payload_detail import bounded_payload, complete_payload, content_page, mask_payloads, parse_text, payload_masking, visible_context_record


class PayloadDetailTests(unittest.TestCase):
    def test_request_masking_preserves_guards_and_restores_nested_context(self):
        value = {'password': 'PRIVATE', 'encoded': '{"authorization":"SECRET","count":0}', 'text': 'api_key=SECRET', 'large': 'x'*50000}
        self.assertTrue(payload_masking.get())
        self.assertNotIn('PRIVATE', json.dumps(complete_payload(value)))
        with mask_payloads(False):
            self.assertEqual(complete_payload(value)['value']['password'], 'PRIVATE')
            self.assertEqual(complete_payload(value)['value']['encoded']['authorization'], 'SECRET')
            self.assertEqual(complete_payload(value)['value']['text'], 'api_key=SECRET')
            bounded = bounded_payload(value)
            self.assertEqual(bounded['value']['password'], 'PRIVATE')
            self.assertTrue(bounded['truncated'])
            with mask_payloads(True):
                self.assertNotIn('PRIVATE', json.dumps(bounded_payload(value)))
            self.assertFalse(payload_masking.get())
        self.assertTrue(payload_masking.get())
        with self.assertRaises(RuntimeError):
            with mask_payloads(False):
                raise RuntimeError('Fixture')
        self.assertTrue(payload_masking.get())
        with self.assertRaises(ValueError):
            with mask_payloads('false'):
                pass

    def test_masking_does_not_leak_between_threads_or_disable_metadata_redaction(self):
        from local_activity_monitor.operation_records import redact
        def read_default():
            return payload_masking.get(), complete_payload({'password': 'PRIVATE'})['value']
        with ThreadPoolExecutor(max_workers=1) as pool, mask_payloads(False):
            self.assertEqual(pool.submit(read_default).result(), (True, {'password': '[已隱藏]'}))
            self.assertEqual(redact({'password': 'PRIVATE'}), {'password': '[已隱藏]'})
        self.assertTrue(payload_masking.get())

    def test_independent_contexts_on_one_thread_keep_their_own_masking(self):
        unmasked, masked = copy_context(), copy_context()
        unmasked.run(payload_masking.set, False)
        self.assertEqual(unmasked.run(complete_payload, {'password': 'PRIVATE'})['value']['password'], 'PRIVATE')
        self.assertEqual(masked.run(complete_payload, {'password': 'PRIVATE'})['value']['password'], '[已隱藏]')
        self.assertEqual(unmasked.run(complete_payload, {'password': 'PRIVATE'})['value']['password'], 'PRIVATE')
        self.assertTrue(payload_masking.get())

    def test_content_revisions_reject_pages_with_another_masking_mode(self):
        first = content_page('ordinary output')
        with mask_payloads(False):
            with self.assertRaises(ValueError):
                content_page('ordinary output', revision=first['revision'])
            unmasked = content_page('ordinary output')
            self.assertEqual(content_page('ordinary output', revision=unmasked['revision'])['text'], 'ordinary output')
        with self.assertRaises(ValueError):
            content_page('ordinary output', revision=unmasked['revision'])

    def test_context_loader_never_projects_hidden_reasoning_even_when_unmasked(self):
        item = {'type': 'reasoning', 'text': 'PRIVATE FULL REASONING', 'encrypted_content': 'PRIVATE ENCRYPTED', 'content': [{'type': 'reasoning_text', 'text': 'PRIVATE'}], 'summary': [{'type': 'summary_text', 'text': 'PUBLIC SUMMARY'}, {'type': 'reasoning_text', 'text': 'PRIVATE'}]}
        for mask in (True, False):
            with mask_payloads(mask):
                result = visible_context_record({'type': 'response_item', 'payload': item})
                self.assertIn('PUBLIC SUMMARY', json.dumps(result))
                self.assertNotIn('PRIVATE', json.dumps(result))
                for hidden in ({'type': 'reasoning_text', 'text': 'PRIVATE'}, {'type': 'reasoning.text', 'text': 'PRIVATE'}, item | {'channel': 'analysis'}, item | {'summary': []}):
                    self.assertIsNone(visible_context_record({'type': 'response_item', 'payload': hidden}))
                self.assertEqual(complete_payload({'type': 'reasoning', 'text': 'legitimate tool field'})['value'], {'type': 'reasoning', 'text': 'legitimate tool field'})

    def test_visible_context_accepts_only_recorded_public_message_parts(self):
        for role in ('user', 'assistant'):
            item = {'type': 'message', 'role': role, 'channel': 'final', 'content': [{'type': 'output_text', 'text': 'PUBLIC'}, {'type': 'reasoning_text', 'text': 'PRIVATE'}], 'encrypted_content': 'PRIVATE'}
            result = visible_context_record({'type': 'response_item', 'payload': item})
            self.assertEqual(result['role'], role)
            self.assertNotIn('PRIVATE', json.dumps(result))
            self.assertIsNone(visible_context_record({'type': 'response_item', 'payload': item | {'channel': 'analysis'}}))
            self.assertIsNone(visible_context_record({'type': 'response_item', 'payload': item | {'role': 'developer'}}))
        self.assertIsNone(visible_context_record(None))
        self.assertIsNone(visible_context_record({'type': 'event_msg', 'payload': item}))

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
