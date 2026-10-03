"""Publication regressions exercise real paths and the actual bootstrap writer."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "initial_report", Path(__file__).with_name("report.py")
)
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.prior_root = report.ROOT
        report.ROOT = self.root
        (self.root / "local").mkdir()

    def tearDown(self):
        report.ROOT = self.prior_root
        self.directory.cleanup()

    def test_destination_is_required_before_any_input_or_output_access(self):
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                report.main(["--local", str(self.root / "missing")])
        self.assertEqual(list((self.root / "local").iterdir()), [])

    def test_existing_directory_is_rejected_without_changing_its_bytes(self):
        output = self.root / "local/existing"
        output.mkdir()
        sentinel = output / "keep.json"
        sentinel.write_bytes(b"keep existing metadata")
        with self.assertRaises(FileExistsError):
            report.summarize(self.root / "missing", output)
        self.assertEqual(sentinel.read_bytes(), b"keep existing metadata")
        self.assertEqual(list(output.iterdir()), [sentinel])

    def test_output_cannot_target_repository_or_external_directories(self):
        (self.root / "parity").mkdir()
        for output in [
            self.root / "parity/new",
            self.root / "new",
            self.root.with_name(self.root.name + "-external"),
        ]:
            with self.assertRaises(ValueError):
                report.new_output_directory(output)
            self.assertFalse(output.exists())

    def test_individual_file_publication_never_overwrites_an_occupied_file(self):
        output = self.root / "local/occupied.json"
        output.write_bytes(b"prior verified receipt")
        with self.assertRaises(FileExistsError):
            report.write(output, {"replace": True})
        self.assertEqual(output.read_bytes(), b"prior verified receipt")

    def test_actual_generator_stages_files_and_preserves_current_metadata(self):
        inputs = self.root / "local/inputs"
        inputs.mkdir()
        fixtures = {
            "baseline.json": {"content_fingerprint": "engineering-fixture", "files": []},
            "census-with-scripts.json": {
                "plugins": [], "archives": [],
                "cross_archive_path_collisions": 0, "integrity_failures": 0,
            },
            "archive-comparison.json": [],
            "plugin-oracle.json": [],
            "resolution-final.json": {
                "explicit_load_order": [], "unique_definitions": 0,
                "overridden_definitions": 0, "script_links": {},
            },
            "import-plan.json": {"jobs": [], "unsupported": [], "runtime_ready": False},
            "cargo-metadata.json": {"packages": []},
        }
        for name, value in fixtures.items():
            (inputs / name).write_text(json.dumps(value), encoding="utf-8")
        current = {}
        for name in [
            "profiles/manifest.json", "parity/requirements.json",
            "parity/commands.json", "parity/conditions.json", "reports/corpus.json",
        ]:
            path = self.root / name
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(b"current verified metadata")
            current[path] = path.read_bytes()
        source_bytes = {p: p.read_bytes() for p in inputs.iterdir()}
        output = self.root / "local/new-initial-census"
        with contextlib.redirect_stdout(io.StringIO()):
            report.summarize(inputs, output)
        self.assertEqual(len(list(output.rglob("*.json"))), 7)
        staged_commands = json.loads((output / "parity/commands.json").read_text())
        staged_ledger = json.loads((output / "parity/requirements.json").read_text())
        self.assertFalse(staged_commands["opcode_decoder_implemented"])
        self.assertIsNone(staged_ledger["last_verified_revision"])
        for path, value in {**current, **source_bytes}.items():
            self.assertEqual(path.read_bytes(), value)
        with self.assertRaises(FileExistsError):
            report.summarize(inputs, output)
        for path, value in current.items():
            self.assertEqual(path.read_bytes(), value)


if __name__ == "__main__":
    unittest.main()
