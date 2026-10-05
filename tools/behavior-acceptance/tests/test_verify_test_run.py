from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import verify_test_run as verifier


HEAD = "f0e116c14ea7928c0c28863c6d9665d2c14bbe1f"
CASES = [
    ("invalid_quantity_wrong_owner_same_destination_and_missing_lot_preserve_complete_state", "ok"),
    ("late_uninitialized_destination_refuses_transaction_and_does_not_consume_request_identity", "ok"),
    ("transfer_acknowledgements_expire_after_scene_change_or_equal_revision_restore", "ok"),
    ("scene_cancellation_and_new_host_after_restore_expire_selections_even_at_equal_revision", "ok"),
    ("stale_revision_and_reused_request_with_different_selection_refuse_without_effects", "ok"),
    ("source_qualified_input_commits_one_revision_preserves_facts_and_replays_one_receipt", "ok"),
    ("request_and_receipt_capacity_refusals_preserve_state_and_keep_existing_retry_available", "ok"),
    ("transfer_acknowledgements_bind_retained_receipts_and_the_current_commit", "ok"),
    ("missing_deleted_wrong_kind_and_unqualified_owner_sources_remain_refusals", "ok"),
]


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class FrozenTestRunTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.producer_root = self.root / "producer"
        self.evidence_root = self.root / "evidence"
        self.producer_root.mkdir()
        self.evidence_root.mkdir()
        source_dir = self.producer_root / "crates/fallout-runtime/tests"
        source_dir.mkdir(parents=True)
        (self.producer_root / "frozen").mkdir()
        self.source = source_dir / "application_inventory.rs"
        self.source.write_bytes(b"source pin")
        self.binary = self.producer_root / "frozen/application_inventory.exe"
        self.binary.write_bytes(b"immutable test binary")
        self.package_path = self.producer_root / "package.json"
        package = {
            "schema_version": 1,
            "task_id": "fixture-transfer-ack",
            "head": HEAD,
            "source_pins": [{
                "path": "crates/fallout-runtime/tests/application_inventory.rs",
                "sha256": _sha(self.source.read_bytes()),
                "bytes": self.source.stat().st_size,
            }],
            "artifacts": [{
                "path": "frozen/application_inventory.exe",
                "sha256": _sha(self.binary.read_bytes()),
                "bytes": self.binary.stat().st_size,
            }],
        }
        self._write_json(self.package_path, package)
        package_sha = _sha(self.package_path.read_bytes())

        self.stdout_path = self.evidence_root / "stdout.log"
        self.stderr_path = self.evidence_root / "stderr.log"
        self.stdout_path.write_text(self._stdout(CASES), encoding="utf-8")
        self.stderr_path.write_bytes(b"")
        self.receipt_path = self.evidence_root / "receipt.json"
        binary_sha = _sha(self.binary.read_bytes())
        self.receipt = {
            "schema_version": 1,
            "producer_head": HEAD,
            "producer_package_sha256": package_sha,
            "source_pins_verified": True,
            "binary": str(self.binary),
            "binary_sha256_before": binary_sha,
            "binary_sha256_after": binary_sha,
            "argv": [
                "py", "-3", "G:\\Rust-Fallout\\tools\\team-build.py",
                "--lane", "behavior-acceptance", "--session",
                "ada80269-206e-45b2-b993-96cb52482f9e", "--wait", "--",
                str(self.binary), "--nocapture",
            ],
            "wrapper_exit_code": 0,
            "cargo_invoked": False,
            "stdout_path": str(self.stdout_path),
            "stdout_sha256": _sha(self.stdout_path.read_bytes()),
            "stdout_bytes": self.stdout_path.stat().st_size,
            "stderr_path": str(self.stderr_path),
            "stderr_sha256": _sha(self.stderr_path.read_bytes()),
            "stderr_bytes": 0,
        }
        self._write_json(self.receipt_path, self.receipt)
        self.expectation_path = self.evidence_root / "expectation.json"
        self.expectation = {
            "format": verifier.FORMAT,
            "schema_version": 1,
            "scenario_id": "frozen-inventory-acknowledgment-cases",
            "lane": "behavior-acceptance",
            "producer_head": HEAD,
            "producer_package_sha256": package_sha,
            "binary_relative_path": "frozen/application_inventory.exe",
            "binary_sha256": binary_sha,
            "test_cases": [{"name": name, "status": status} for name, status in CASES],
            "require_empty_stderr": True,
        }
        self._write_json(self.expectation_path, self.expectation)

    def tearDown(self):
        self.temporary.cleanup()

    @staticmethod
    def _stdout(cases):
        lines = [
            "behavior-acceptance: focused slot acquired (2/2)",
            f"running {len(cases)} tests",
        ]
        for name, status in cases:
            rendered = "ignored, test helper" if status == "ignored" else status
            lines.append(f"test {name} ... {rendered}")
        passed = sum(status == "ok" for _, status in cases)
        failed = sum(status == "failed" for _, status in cases)
        ignored = sum(status == "ignored" for _, status in cases)
        overall = "FAILED" if failed else "ok"
        lines.append(
            f"test result: {overall}. {passed} passed; {failed} failed; "
            f"{ignored} ignored; 0 measured; 0 filtered out; finished in 0.04s"
        )
        return "\n".join(lines) + "\n"

    @staticmethod
    def _write_json(path, value):
        path.write_text(json.dumps(value), encoding="utf-8")

    def _refresh_stdout(self, cases):
        self.stdout_path.write_text(self._stdout(cases), encoding="utf-8")
        self.receipt["stdout_sha256"] = _sha(self.stdout_path.read_bytes())
        self.receipt["stdout_bytes"] = self.stdout_path.stat().st_size
        self._write_json(self.receipt_path, self.receipt)

    def _verify(self):
        return verifier.verify(
            self.package_path,
            self.receipt_path,
            self.expectation_path,
            self.producer_root,
            self.evidence_root,
        )

    def test_pinned_frozen_inventory_result_passes_without_retail_claim(self):
        result = self._verify()
        self.assertEqual(result["status"], "passed", result)
        self.assertEqual(result["classification"], "frozen_test_artifact_validation")
        self.assertIs(result["retail_pass"], False)
        self.assertEqual(result["test_counts"], {"passed": 9, "failed": 0, "ignored": 0})

    def test_missing_or_renamed_acknowledgment_case_fails_expectation(self):
        self._refresh_stdout(CASES[:-1])
        result = self._verify()
        self.assertEqual(result["status"], "failed")
        self.assertTrue(any(m.get("missing") for m in result["mismatches"]), result)

        renamed = list(CASES)
        renamed[-1] = ("acknowledgment_result_was_substituted", "ok")
        self._refresh_stdout(renamed)
        result = self._verify()
        self.assertEqual(result["status"], "failed")
        self.assertTrue(
            any("acknowledgment_result_was_substituted" in m.get("unexpected", []) for m in result["mismatches"]),
            result,
        )

    def test_failed_consumer_case_is_not_accepted(self):
        cases = list(CASES)
        cases[1] = (cases[1][0], "failed")
        self._refresh_stdout(cases)
        self.receipt["wrapper_exit_code"] = 1
        self._write_json(self.receipt_path, self.receipt)
        result = self._verify()
        self.assertEqual(result["status"], "failed")
        self.assertTrue(
            any(m.get("path") == "/test_results" for m in result["mismatches"]),
            result,
        )

    def test_missing_or_unknown_wrapper_exit_is_rejected(self):
        known_exit = self.receipt.pop("wrapper_exit_code")
        self._write_json(self.receipt_path, self.receipt)
        result = self._verify()
        self.assertEqual(result["classification"], "invalid_receipt", result)

        for unknown_exit in (None, True, "0"):
            with self.subTest(wrapper_exit_code=unknown_exit):
                self.receipt["wrapper_exit_code"] = unknown_exit
                self._write_json(self.receipt_path, self.receipt)
                result = self._verify()
                self.assertEqual(result["classification"], "invalid_receipt", result)

        self.receipt["wrapper_exit_code"] = 75
        self._write_json(self.receipt_path, self.receipt)
        result = self._verify()
        self.assertEqual(result["status"], "failed", result)
        self.assertTrue(
            any(m.get("path") == "/wrapper_exit_code" for m in result["mismatches"]),
            result,
        )
        self.receipt["wrapper_exit_code"] = known_exit

    def test_duplicate_case_and_summary_disagreement_are_invalid_receipts(self):
        self.stdout_path.write_text(
            self.stdout_path.read_text(encoding="utf-8")
            + f"test {CASES[0][0]} ... ok\n",
            encoding="utf-8",
        )
        self.receipt["stdout_sha256"] = _sha(self.stdout_path.read_bytes())
        self.receipt["stdout_bytes"] = self.stdout_path.stat().st_size
        self._write_json(self.receipt_path, self.receipt)
        result = self._verify()
        self.assertEqual(result["classification"], "invalid_receipt")

    def test_package_source_binary_and_log_pins_are_enforced(self):
        self.source.write_bytes(b"changed source")
        self.assertEqual(self._verify()["classification"], "invalid_receipt")

        self.source.write_bytes(b"source pin")
        self.binary.write_bytes(b"changed binary")
        self.assertEqual(self._verify()["classification"], "invalid_receipt")

        self.binary.write_bytes(b"immutable test binary")
        self.stdout_path.write_text("tampered\n", encoding="utf-8")
        self.assertEqual(self._verify()["classification"], "invalid_receipt")

    def test_noncentral_or_rebuilt_run_is_rejected(self):
        self.receipt["cargo_invoked"] = True
        self._write_json(self.receipt_path, self.receipt)
        self.assertEqual(self._verify()["classification"], "invalid_receipt")

        self.receipt["cargo_invoked"] = False
        self.receipt["argv"] = ["application_inventory.exe", "--nocapture"]
        self._write_json(self.receipt_path, self.receipt)
        self.assertEqual(self._verify()["classification"], "invalid_receipt")


if __name__ == "__main__":
    unittest.main()
