"""Check byte offsets, JSON compatibility and counters in both runtimes."""
import json
import unittest

from local_activity_monitor import session_scan


def collected(result):
    *state, rows = result
    return (*state, list(rows))


class SessionScanTests(unittest.TestCase):
    def test_partial_unicode_line_resumes_at_its_original_offset(self):
        body = '{"文字":0}'.encode()
        skipped, tail, discard, rows = session_scan.forward(b"{}\n" + body[:4], False, 20)
        self.assertEqual((skipped, tail, discard, list(rows)), (0, body[:4], False, [(0, b"{}")]))
        self.assertEqual(collected(session_scan.forward(tail + body[4:] + b"\r\n", discard, 20)),
                         (0, b"", False, [(0, body + b"\r")]))

    def test_oversized_line_discards_until_lf_and_keeps_later_offsets(self):
        self.assertEqual(collected(session_scan.forward(b"12345", False, 4)), (0, b"", True, []))
        self.assertEqual(collected(session_scan.forward(b"tail", True, 4)), (0, b"tail", True, []))
        self.assertEqual(collected(session_scan.forward(b"tail\n\n12345\n1234\np", True, 4)),
                         (5, b"p", False, [(12, b"1234")]))

    def test_backfill_retains_unterminated_tail_and_skips_boundary_fragment(self):
        data = b"frag\n\n{}\r\nx"
        self.assertEqual(collected(session_scan.backfill(data, True, 4, reverse=False)),
                         (5, [(6, b"{}\r"), (10, b"x")]))
        self.assertEqual(collected(session_scan.backfill(data, True, 4, reverse=True)),
                         (5, [(10, b"x"), (6, b"{}\r")]))
        self.assertEqual(collected(session_scan.backfill(b"frag", True, 4, reverse=True)), (5, []))
        self.assertEqual(collected(session_scan.backfill(b"frag", False, 4, reverse=True)), (0, [(0, b"frag")]))

    @unittest.skipIf(session_scan._native is None, "Rust session scanner is not built")
    def test_native_matches_python_at_every_chunk_boundary(self):
        body = b"\n" + '{"文字":0}'.encode() + b"\r\n\x00\xff\n" + b"x" * 25 + b"\n{}\npartial"
        for split in range(len(body) + 1):
            for chunk in (body[:split], body[split:]):
                for skip in (False, True):
                    with self.subTest(split=split, chunk=chunk, skip=skip):
                        self.assertEqual(collected(session_scan.forward(chunk, skip, 20)),
                                         collected(session_scan._forward(chunk, skip, 20)))
                        for reverse in (False, True):
                            self.assertEqual(collected(session_scan.backfill(chunk, skip, 20, reverse)),
                                             collected(session_scan._backfill(chunk, skip, 20, reverse)))


class SessionRecordTests(unittest.TestCase):
    @unittest.skipIf(session_scan._decode is None, "Rust session decoder is not built")
    def test_native_decodes_types_and_preserves_duplicate_key_order(self):
        raw = (
            '{"z":0,"a":true,"z":null,"unicode":"繁體\\ud83d\\ude00",'
            '"data":[-0,-0.0,1.0000000000000002,5e-324,9223372036854775807]}'
        ).encode()
        expected = json.loads(raw)
        native = session_scan._decode(raw)
        self.assertEqual(json.dumps(native), json.dumps(expected))
        self.assertEqual(json.dumps(session_scan._decode(raw.decode())), json.dumps(expected))
        self.assertIsNone(session_scan._decode(b"null"))
        self.assertIs(session_scan._decode(b"true"), True)

    def test_compatibility_inputs_keep_cpython_decoding(self):
        cases = (
            b'{"n":9223372036854775808,"m":-9223372036854775809}',
            b'{"value":"\\ud800"}',
            b'{"value":"\xed\xa0\x80"}',
            b'\xef\xbb\xbf{"value":1}',
            '{"value":1}'.encode("utf-16"),
            '{"value":1}'.encode("utf-32"),
            b'{"value":[NaN,Infinity,-Infinity]}',
            '{"value":"\ud800"}',
            b"[" * 250 + b"null" + b"]" * 250,
        )
        for raw in cases:
            with self.subTest(raw=raw[:50]):
                self.assertEqual(json.dumps(session_scan.decode_record(raw)), json.dumps(json.loads(raw)))

    def test_bad_records_keep_cpython_error(self):
        for raw in (b"", b'{"x":1,}', b'{"x":"\xff"}', b"{} {}"):
            with self.subTest(raw=raw):
                with self.assertRaises(ValueError) as expected:
                    json.loads(raw)
                with self.assertRaises(type(expected.exception)) as result:
                    session_scan.decode_record(raw)
                self.assertEqual(str(result.exception), str(expected.exception))

    def test_token_projection_keeps_known_zero_and_rejects_other_values(self):
        class IntSubclass(int):
            pass

        keys = ("zero", "max", "too_large", "negative", "bool", "float", "missing", "subclass")
        usage = {
            "zero": 0, "max": 2**63 - 1, "too_large": 2**63, "negative": -1, "bool": True,
            "float": 0.0, "subclass": IntSubclass(1), "unselected": 123,
        }
        expected = dict.fromkeys(keys) | {"zero": 0, "max": 2**63 - 1}
        self.assertEqual(session_scan.project_counters(usage, keys), expected)
        self.assertIsNone(session_scan.project_counters({"bool": False}, keys))
