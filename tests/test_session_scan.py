"""Offsets remain byte-based across source chunks and scanner implementations."""
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
