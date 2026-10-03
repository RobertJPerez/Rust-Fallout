"""Check the boundaries that keep source-lane receipts tied to real inputs."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("source_lanes", Path(__file__).with_name("verify-source-lanes.py"))
lanes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lanes)


class SourceLaneGuards(unittest.TestCase):
    def test_a_well_formed_changed_digest_does_not_bind_the_oracle(self):
        lanes.binary_identity({"oracle_binary_sha256": "a" * 64}, "a" * 64)
        with self.assertRaisesRegex(RuntimeError, "different binary"):
            lanes.binary_identity({"oracle_binary_sha256": "b" * 64}, "a" * 64)

    def test_missing_binary_identity_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "different binary"):
            lanes.binary_identity({}, "a" * 64)

    def test_evidence_must_be_new_and_immediately_under_the_owned_local_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "local").mkdir()
            self.assertEqual(lanes.new_local_path(root, Path("local/new")), root / "local/new")
            for path in [Path("outside"), Path("local/../outside"), Path("local/nested/new")]:
                with self.assertRaises(RuntimeError):
                    lanes.new_local_path(root, path)
            (root / "local/used").mkdir()
            with self.assertRaisesRegex(RuntimeError, "already exists"):
                lanes.new_local_path(root, Path("local/used"))


if __name__ == "__main__":
    unittest.main()
