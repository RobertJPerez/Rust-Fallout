"""Focused guards for the proof-only container transport and extraction."""
import hashlib
from pathlib import Path
import struct
import unittest

from integration_actor_context_check import authored_envelope, extract_state


FIXTURE = Path(r"G:\Rust-Fallout-worktrees\v3-actors\local\v3-act02-packages-20261004-01\export-03\authored-packages\snapshot.json")


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.body = FIXTURE.read_bytes()
        self.wire = authored_envelope(self.body)

    def test_exact_historical_body_survives_without_reserialization(self):
        self.assertEqual(extract_state(self.wire, 3), self.body)
        self.assertEqual(len(self.wire), len(self.body) + 232)

    def test_wrong_schema_is_not_silently_relabelled(self):
        with self.assertRaisesRegex(ValueError, "META schema"):
            extract_state(self.wire, 4)

    def test_rehashed_outer_digest_does_not_hide_chunk_corruption(self):
        wire = bytearray(self.wire)
        wire[201] ^= 1
        wire[-32:] = hashlib.sha256(wire[:-32]).digest()
        with self.assertRaisesRegex(ValueError, "chunk digest"):
            extract_state(bytes(wire), 3)

    def test_valid_hashes_do_not_hide_disagreeing_metadata(self):
        wire = bytearray(self.wire)
        struct.pack_into("<Q", wire, 64 + 80, 999)
        wire[32:64] = hashlib.sha256(wire[64:152]).digest()
        wire[-32:] = hashlib.sha256(wire[:-32]).digest()
        with self.assertRaisesRegex(ValueError, "META revision"):
            extract_state(bytes(wire), 3)

    def test_trailing_bytes_with_valid_outer_hash_refuse(self):
        wire = self.wire[:-32] + b"x"
        wire += hashlib.sha256(wire).digest()
        with self.assertRaisesRegex(ValueError, "trailing"):
            extract_state(wire, 3)


if __name__ == "__main__":
    unittest.main()
