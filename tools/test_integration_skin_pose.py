"""Proof-driver admission/provenance tests; these do not build or accept an engine."""
import argparse
import copy
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest import mock

import integration_skin_pose as proof


def commit(root, files):
    root.mkdir()
    subprocess.run(["git", "init", "--quiet", str(root)], check=True)
    for name, data in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    subprocess.run(["git", "-C", str(root), "add", "."], check=True)
    subprocess.run(["git", "-C", str(root), "-c", "user.name=Proof tests",
                    "-c", "user.email=proof-tests@example.invalid", "commit", "--quiet", "-m", "fixture"], check=True)
    return proof.git(root, "rev-parse", "HEAD").decode().strip()


class SimulatedCommands(proof.Commands):
    """Only build/producer stages are simulated; private Git freezes run normally."""
    fault = None

    def execute(self, name, arguments, cwd, stdout=None):
        arguments = [str(value) for value in arguments]
        if name.endswith(("-archive", "-clone", "-checkout")):
            return super().execute(name, arguments, cwd, stdout)
        self.rows.append({"name": name, "arguments": arguments, "cwd": str(cwd),
                          "exit_code": 0, "simulated_for_tests": True})
        option = lambda flag: Path(arguments[arguments.index(flag) + 1])
        if self.fault == "build-failure" and name == "producer-build":
            self.rows[-1]["exit_code"] = 17
            raise ValueError("producer-build failed with exit 17")
        if name == "producer-build":
            if self.fault == "cargo-mutated-during-producer-build":
                Path(arguments[0]).write_bytes(b"changed Cargo during producer build")
            path = Path(self.environment["CARGO_TARGET_DIR"]) / "release/fallout.exe"
            path.parent.mkdir()
            path.write_bytes(b"test producer; never executed")
        elif name == "native-build":
            if self.fault == "producer-mutated-during-native-build":
                (self.output / "binaries/fallout.exe").write_bytes(b"changed frozen producer during native build")
            path = option("-BuildDirectory") / "Release/nif-skin-oracle.exe"
            path.parent.mkdir()
            path.write_bytes(b"test native reader; never executed")
        elif name == "native-source":
            selected, binary = option("--input"), option("--binary")
            self.native_row = {"file": selected.name, "sha256": proof.identity(selected)["sha256"],
                "decoded_bytes": selected.stat().st_size,
                "owners": [{"geometry": 1, "instance": 3, "vertex_count": 1}],
                "skins": [{"block": 3, "data": {"instance": {"bones": [5]}}}]}
            report = {"schema_version": 3, "nifly_revision": proof.NIFLY_REVISION,
                "float_encoding": "ieee754-binary32-bits", "binding_scope": "decoded-source-forest",
                "partition_branch": "nv-canonical-flags-four-wide-or-empty", "prepare_data_called": False,
                "raw_presence_and_vertex_counts_checked": True, "raw_partition_fields_checked": True,
                "raw_node_fields_checked": True, "graph_membership_checked": True,
                "oracle_binary_sha256": proof.identity(binary)["sha256"], "files": [self.native_row]}
            if self.fault == "wrong-native-binary":
                report["oracle_binary_sha256"] = "0" * 64
            if self.fault == "native-row-omitted":
                report["files"] = []
            proof.write_json(option("--output"), report)
        elif name == "source-comparison":
            selected = Path(arguments[2])
            native = option("--oracle-report")
            report = {"schema_version": 3, "failures": 0, "runtime_ready": False,
                "binding_scope": "decoded-source-forest", "oracle_report_sha256": proof.identity(native)["sha256"],
                "oracle_binary_sha256": proof.identity(self.output / "binaries/nif-skin-oracle.exe")["sha256"],
                "files": [{"input": str(selected), "sha256": proof.identity(selected)["sha256"],
                           "decoded_bytes": selected.stat().st_size, "comparison": "all_equal", "error": None}]}
            if self.fault == "comparison-not-exact":
                report["files"][0]["comparison"] = None
            if self.fault == "comparison-foreign-reference":
                report["oracle_report_sha256"] = "0" * 64
            proof.write_json(option("--output"), report)
        elif name == "rational-pose":
            directory, selected = option("--output-dir"), option("--input")
            binary, native = option("--binary"), option("--native-report")
            directory.mkdir()
            if self.fault == "native-report-mutated-during-pose":
                native.write_bytes(native.read_bytes() + b"\n")
            if self.fault == "comparison-mutated-during-pose":
                path = self.output / "source-comparison.json"
                path.write_bytes(path.read_bytes() + b"\n")
            if self.fault == "command-log-mutated-during-pose":
                path = self.output / "rust-clone.stdout.txt"
                path.write_bytes(path.read_bytes() + b"later stdout")
            tolerance = float(arguments[arguments.index("--tolerance") + 1])
            rows = []
            for label, geometry, tol, code in (("positive", 1, tolerance, 0),
                    ("strict-zero-tolerance", 1, 0.0, 1), ("unowned-geometry", 2**32-1, tolerance, 1)):
                path = directory / (label + ".json")
                proof.write_json(path, {"simulated_for_tests": True, "failures": code})
                (directory / (label + ".stdout.txt")).write_bytes(b"pose stdout")
                (directory / (label + ".stderr.txt")).write_bytes(b"")
                rows.append({"command": [str(binary.resolve()), "nif-skin", str(selected.resolve()),
                    "--pose-geometry", str(geometry), "--pose-weight-tolerance", str(tol),
                    "--output", str(path.resolve())], "exit_code": code,
                    "receipt_sha256": proof.identity(path)["sha256"]})
            report = {"contract": proof.CONTRACT, "binary_sha256": proof.identity(binary)["sha256"],
                "source_sha256": proof.identity(selected)["sha256"], "native_report_sha256": proof.identity(native)["sha256"],
                "retail_behavior_verified": False, "intended_refusals": 2, "altered_palette_rejections": 1,
                "numeric": {"vertices": 1, "palette_entries": 1, "coefficients": 12,
                    "maximum_absolute_palette_error": 0.0,
                    "numerical_bound": "gamma_(7*m) * absolute matrix product + 2^-1074"},
                "invocations": rows}
            if self.fault == "pose-retail-claim":
                report["retail_behavior_verified"] = True
            if self.fault == "pose-missing-refusal":
                report["invocations"].pop()
            if self.fault == "pose-receipt-tampered":
                rows[1]["receipt_sha256"] = "0" * 64
            if self.fault == "pose-wrong-coverage":
                report["numeric"]["coefficients"] = 0
            proof.write_json(directory / "summary.json", report)
            if self.fault == "frozen-binary-mutated":
                binary.write_bytes(b"changed after proof")
            if self.fault == "original-input-mutated":
                self.original_input.write_bytes(b"changed during proof")


class DriverTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="skin-proof-driver-tests-", dir=Path(__file__).parent)
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        self.native = self.folder / "upstream"
        self.native_revision = commit(self.native, {"CMakeLists.txt": b"fixture\n", "Skin.cpp": b"source\n"})
        self.pin = mock.patch.object(proof, "NIFLY_REVISION", self.native_revision)
        self.pin.start()
        self.addCleanup(self.pin.stop)
        self.root = self.folder / "repository"
        self.revision = commit(self.root, {".gitignore": b"/local/\n",
            "sources.lock.json": json.dumps({"references": [{"url": "https://github.com/ousnius/nifly",
                                                            "commit": self.native_revision}]}).encode(),
            "Cargo.toml": b"fixture\n"})
        (self.root / "local").mkdir()
        self.source = self.folder / "selected.blob"
        self.source.write_bytes(b"selected source, no parser in these tests")
        self.package = self.folder / "input-package.json"
        self.value = {"schema_version": 1, "input": {"path": str(self.source), **proof.identity(self.source)},
            "geometry": 1, "absolute_weight_tolerance": 1.1920928955078125e-7,
            "native_source": {"path": str(self.native), "revision": self.native_revision,
                              "files": proof.source_snapshot(self.native, self.native_revision)["files"]}}
        self.cargo = self.folder / "cargo.exe"
        self.cargo.write_bytes(b"test cargo; never executed")
        self.save_package()

    def save_package(self):
        self.package.write_text(json.dumps(self.value), encoding="utf-8")
        self.args = argparse.Namespace(repository=self.root, revision=self.revision,
            output_directory=self.root / "local/proof", input_package=self.package,
            input_package_sha256=proof.identity(self.package)["sha256"],
            input_package_bytes=self.package.stat().st_size, cargo=self.cargo,
            cargo_home=self.folder, rustup_home=self.folder)

    def execute(self, fault=None):
        class Runner(SimulatedCommands):
            pass
        Runner.fault, Runner.original_input = fault, self.source
        original_snapshot = proof.source_snapshot
        late_paths = {"late-pose-receipt": "pose/positive.json", "late-pose-summary": "pose/summary.json",
                      "late-pose-stdout": "pose/positive.stdout.txt", "late-pose-stderr": "pose/positive.stderr.txt",
                      "late-command-started": "rust-clone.started.json",
                      "late-command-completed": "rust-clone.command.json",
                      "late-command-stdout": "rust-clone.stdout.txt",
                      "late-command-stderr": "rust-clone.stderr.txt"}
        changed = False

        def final_snapshot(root, revision):
            nonlocal changed
            if (not changed and fault in late_paths and root == self.root
                    and (self.args.output_directory / "pose/summary.json").exists()):
                path = self.args.output_directory / late_paths[fault]
                path.write_bytes(path.read_bytes() + b"\n")
                changed = True
            return original_snapshot(root, revision)

        with mock.patch.object(proof, "source_snapshot", side_effect=final_snapshot):
            code = proof.run(self.args, Runner)
        return code, proof.document(self.args.output_directory / "receipt.json")

    def test_simulated_stages_bind_source_binary_input_and_independent_comparison(self):
        code, receipt = self.execute()
        self.assertEqual(code, 0)
        self.assertTrue(receipt["engineering_proof_passed"])
        self.assertFalse(receipt["retail_behavior_verified"])
        self.assertFalse(receipt["playback_verified"])
        self.assertEqual(receipt["source_before"], receipt["source_after"])
        self.assertEqual(receipt["frozen_source_before"], receipt["frozen_source_after"])
        self.assertEqual(receipt["binaries_before"], receipt["binaries_after"])
        self.assertEqual(receipt["inputs_before"], receipt["inputs_after"])
        self.assertEqual(receipt["evidence_before"], receipt["evidence_after"])
        names = [row["name"] for row in receipt["commands"]]
        self.assertLess(names.index("source-comparison"), names.index("rational-pose"))
        build = next(row for row in receipt["commands"] if row["name"] == "producer-build")
        self.assertIn("--locked", build["arguments"])
        self.assertIn("--offline", build["arguments"])

    def test_existing_output_is_preserved(self):
        self.args.output_directory.mkdir()
        old = self.args.output_directory / "receipt.json"
        old.write_bytes(b"old proof")
        with self.assertRaisesRegex(ValueError, "fresh directory"):
            self.execute()
        self.assertEqual(old.read_bytes(), b"old proof")

    def test_dirty_candidate_refuses_before_builds_and_writes_failure_receipt(self):
        (self.root / "Cargo.toml").write_bytes(b"dirty\n")
        code, receipt = self.execute()
        self.assertEqual(code, 1)
        self.assertIn("clean and committed", receipt["error"])
        self.assertEqual(receipt["commands"], [])

    def test_input_hash_mismatch_refuses_before_builds(self):
        self.source.write_bytes(b"altered selected source")
        code, receipt = self.execute()
        self.assertEqual(code, 1)
        self.assertIn("digest/length differs", receipt["error"])
        self.assertEqual(receipt["commands"], [])

    def test_native_package_omission_refuses_before_builds(self):
        self.value["native_source"]["files"].pop()
        self.save_package()
        code, receipt = self.execute()
        self.assertEqual(code, 1)
        self.assertIn("omits tracked files", receipt["error"])
        self.assertEqual(receipt["commands"], [])

    def test_native_package_escape_refuses_before_builds(self):
        self.value["native_source"]["files"][0]["path"] = "../foreign.cpp"
        self.save_package()
        code, receipt = self.execute()
        self.assertEqual(code, 1)
        self.assertIn("escapes its root", receipt["error"])
        self.assertEqual(receipt["commands"], [])

    def test_wrong_native_revision_refuses_before_builds(self):
        self.value["native_source"]["revision"] = "0" * 40
        self.save_package()
        code, receipt = self.execute()
        self.assertEqual(code, 1)
        self.assertIn("revision differs from pin", receipt["error"])
        self.assertEqual(receipt["commands"], [])

    def test_nonfinite_weight_policy_refuses_before_builds(self):
        self.value["absolute_weight_tolerance"] = float("nan")
        self.save_package()
        code, receipt = self.execute()
        self.assertEqual(code, 1)
        self.assertIn("finite positive", receipt["error"])
        self.assertEqual(receipt["commands"], [])

    def test_actual_command_failure_preserves_raw_bytes_exit_and_stage_receipt(self):
        output = self.root / "local/command-test"
        output.mkdir()
        runner = proof.Commands(output, None)
        program = "import os; os.write(1,b'raw\\xff'); os.write(2,b'error\\x00'); raise SystemExit(7)"
        with self.assertRaisesRegex(ValueError, "failed with exit 7"):
            runner.execute("native-failure", [proof.sys.executable, "-c", program], self.root)
        self.assertEqual((output / "native-failure.stdout.txt").read_bytes(), b"raw\xff")
        self.assertEqual((output / "native-failure.stderr.txt").read_bytes(), b"error\x00")
        receipt = proof.document(output / "native-failure.command.json")
        self.assertEqual(receipt["exit_code"], 7)
        self.assertEqual(receipt["outputs"][0]["sha256"], proof.identity(output / "native-failure.stdout.txt")["sha256"])
        expected = ["native-failure.started.json", "native-failure.stdout.txt",
                    "native-failure.stderr.txt", "native-failure.command.json"]
        self.assertEqual(set(runner.evidence), {str((output / name).resolve()) for name in expected})
        for path, sealed in runner.evidence.items():
            self.assertEqual(sealed, {"path": path, **proof.identity(Path(path))})

    def test_cargo_mutation_during_producer_build_keeps_first_binary_identity(self):
        original = proof.identity(self.cargo)
        code, receipt = self.execute("cargo-mutated-during-producer-build")
        self.assertEqual(code, 1)
        self.assertEqual(receipt["binaries_before"]["cargo"], {"path": str(self.cargo.resolve()), **original})
        self.assertNotEqual(receipt["binaries_before"]["cargo"], receipt["binaries_after"]["cargo"])
        self.assertFalse(receipt["engineering_proof_passed"])

    def test_producer_mutation_during_native_build_keeps_first_binary_identity(self):
        code, receipt = self.execute("producer-mutated-during-native-build")
        self.assertEqual(code, 1)
        expected = proof.hashlib.sha256(b"test producer; never executed").hexdigest()
        self.assertEqual(receipt["binaries_before"]["producer-frozen"]["sha256"], expected)
        self.assertNotEqual(receipt["binaries_before"]["producer-frozen"], receipt["binaries_after"]["producer-frozen"])
        self.assertFalse(receipt["engineering_proof_passed"])

    def test_native_report_change_during_pose_cannot_become_a_new_baseline(self):
        code, receipt = self.execute("native-report-mutated-during-pose")
        self.assertEqual(code, 1)
        path = str((self.args.output_directory / "native.json").resolve())
        self.assertNotEqual(receipt["evidence_before"][path], receipt["evidence_after"][path])
        self.assertFalse(receipt["engineering_proof_passed"])

    def test_source_comparison_change_during_pose_cannot_become_a_new_baseline(self):
        code, receipt = self.execute("comparison-mutated-during-pose")
        self.assertEqual(code, 1)
        path = str((self.args.output_directory / "source-comparison.json").resolve())
        self.assertNotEqual(receipt["evidence_before"][path], receipt["evidence_after"][path])
        self.assertFalse(receipt["engineering_proof_passed"])

    def test_earlier_command_log_change_during_pose_is_rejected(self):
        code, receipt = self.execute("command-log-mutated-during-pose")
        self.assertEqual(code, 1)
        self.assertTrue(receipt["immutability_errors"])
        self.assertFalse(receipt["engineering_proof_passed"])

    def assert_final_evidence_mutation_rejected(self, fault, name):
        code, receipt = self.execute(fault)
        self.assertEqual(code, 1)
        path = str((self.args.output_directory / name).resolve())
        self.assertNotEqual(receipt["evidence_before"][path], receipt["evidence_after"][path])
        self.assertTrue(receipt["immutability_errors"])
        self.assertFalse(receipt["engineering_proof_passed"])

    def test_accepted_pose_receipt_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-pose-receipt", "pose/positive.json")

    def test_accepted_pose_summary_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-pose-summary", "pose/summary.json")

    def test_pose_stdout_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-pose-stdout", "pose/positive.stdout.txt")

    def test_pose_stderr_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-pose-stderr", "pose/positive.stderr.txt")

    def test_command_started_receipt_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-command-started", "rust-clone.started.json")

    def test_command_completed_receipt_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-command-completed", "rust-clone.command.json")

    def test_command_stdout_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-command-stdout", "rust-clone.stdout.txt")

    def test_command_stderr_mutation_in_final_checks_is_rejected(self):
        self.assert_final_evidence_mutation_rejected("late-command-stderr", "rust-clone.stderr.txt")

    def test_failed_build_retains_command_failure_and_source_checks(self):
        code, receipt = self.execute("build-failure")
        self.assertEqual(code, 1)
        self.assertIn("exit 17", receipt["error"])
        self.assertEqual(receipt["source_before"], receipt["source_after"])
        self.assertFalse(receipt["engineering_proof_passed"])
        self.assertNotIn("rational-pose", [row["name"] for row in receipt["commands"]])

    def test_wrong_native_binary_blocks_comparison(self):
        code, receipt = self.execute("wrong-native-binary")
        self.assertEqual(code, 1)
        self.assertIn("different binary", receipt["error"])
        self.assertNotIn("source-comparison", [row["name"] for row in receipt["commands"]])

    def test_incomplete_source_comparison_blocks_rational_checks(self):
        code, receipt = self.execute("comparison-not-exact")
        self.assertEqual(code, 1)
        self.assertIn("compared completely", receipt["error"])
        self.assertNotIn("rational-pose", [row["name"] for row in receipt["commands"]])

    def test_foreign_source_reference_blocks_rational_checks(self):
        code, receipt = self.execute("comparison-foreign-reference")
        self.assertEqual(code, 1)
        self.assertIn("this fresh native", receipt["error"])
        self.assertNotIn("rational-pose", [row["name"] for row in receipt["commands"]])

    def test_refusal_receipt_tampering_is_rejected(self):
        code, receipt = self.execute("pose-receipt-tampered")
        self.assertEqual(code, 1)
        self.assertIn("receipt/command identity differs", receipt["error"])

    def test_missing_refusal_is_rejected(self):
        code, receipt = self.execute("pose-missing-refusal")
        self.assertEqual(code, 1)
        self.assertIn("invocation coverage", receipt["error"])

    def test_invented_numeric_coverage_is_rejected(self):
        code, receipt = self.execute("pose-wrong-coverage")
        self.assertEqual(code, 1)
        self.assertIn("numeric coverage", receipt["error"])

    def test_retail_claim_is_rejected(self):
        code, receipt = self.execute("pose-retail-claim")
        self.assertEqual(code, 1)
        self.assertIn("identities/refusals", receipt["error"])

    def test_frozen_binary_mutation_revokes_success(self):
        code, receipt = self.execute("frozen-binary-mutated")
        self.assertEqual(code, 1)
        self.assertTrue(receipt["immutability_errors"])
        self.assertFalse(receipt["engineering_proof_passed"])

    def test_original_input_mutation_revokes_success(self):
        code, receipt = self.execute("original-input-mutated")
        self.assertEqual(code, 1)
        self.assertTrue(receipt["immutability_errors"])
        self.assertFalse(receipt["engineering_proof_passed"])


class RationalCheckerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Exercise the committed existing checker, without making another parser.
        root = next(path for path in Path(__file__).resolve().parents if (path / ".git").exists())
        blob = proof.git(root, "show", "6ee939236acca69b2465dad2fe281f24f9ddb28f:tools/nif-skin-oracle/check_pose.py")
        cls.checker = {"__name__": "committed_skin_pose_checker"}
        exec(compile(blob, "committed/check_pose.py", "exec"), cls.checker)

    def test_nonidentity_palette_matches_analytic_composition_and_altered_coefficient_refuses(self):
        bits = lambda value: struct.unpack("<I", struct.pack("<f", value))[0]
        def transform(scale, translation):
            return {"rotation_bits": [[bits(int(r == c)) for c in range(3)] for r in range(3)],
                    "scale_bits": bits(scale), "translation_bits": [bits(v) for v in translation]}
        row = {"sha256": "a" * 64, "owners": [{"geometry": 1, "geometry_data": 2,
                "instance": 3, "vertex_count": 1}],
            "skins": [{"block": 3, "data": {"instance": {"data": 4, "skeleton_root": 0, "bones": [5]}}},
                {"block": 4, "data": {"transform": transform(2, [1, 2, 3]),
                    "bones": [{"transform": transform(0.5, [-1, -2, -3]),
                               "weights": [{"vertex": 0, "weight_bits": bits(1)}]}]}}],
            "bindings": {"nodes": [{"block": 5, "parent": 0, "transform": transform(3, [4, 5, 6])}]}}
        # (2I,t123) * (3I,t456) * (.5I,t-1-2-3) = (3I,t3,0,-3).
        evaluation = {"source_sha256": row["sha256"], "geometry": 1, "geometry_data": 2,
            "instance": 3, "skin_data": 4, "skeleton_root": 0, "positions": [[0, 0, 0]],
            "weight_sums": [1.0], "retail_behavior_verified": False,
            "palette": [{"ordinal": 0, "node": 5,
                         "matrix": [[3, 0, 0, 3], [0, 3, 0, 0], [0, 0, 3, -3]]}]}
        result = self.checker["compare_palette"](evaluation, row, 1)
        self.assertEqual(result["coefficients"], 12)
        self.assertEqual(result["maximum_absolute_palette_error"], 0)
        altered = copy.deepcopy(evaluation)
        altered["palette"][0]["matrix"][0][3] += 0.125
        with self.assertRaisesRegex(ValueError, "palette numeric mismatch"):
            self.checker["compare_palette"](altered, row, 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
