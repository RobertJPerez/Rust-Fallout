from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import struct
import tempfile
import time
import unittest
from unittest.mock import patch
import zlib


SCRIPT = Path(__file__).with_name("scene_replay.py")
SPEC = importlib.util.spec_from_file_location("scene_replay", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
scene_replay = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scene_replay)


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        len(payload).to_bytes(4, "big")
        + kind
        + payload
        + (zlib.crc32(kind + payload) & 0xFFFFFFFF).to_bytes(4, "big")
    )


def rgb_png(width: int, height: int, pixels: bytes) -> bytes:
    stride = width * 3
    rows = b"".join(b"\0" + pixels[y * stride : (y + 1) * stride] for y in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", header)
        + png_chunk(b"IDAT", zlib.compress(rows))
        + png_chunk(b"IEND", b"")
    )


def sample_report() -> dict:
    source_root = {"profile": "fixture", "origin_plugin": "fixture.esm", "local_id": 1}
    camera_request = {"source_position": [1.0, 2.0, 3.0], "source_target": [4.0, 5.0, 6.0]}
    return {
        "schema_version": 3,
        "cell": {
            "key": {"origin_plugin": "FalloutNV.esm", "local_id": 123},
            "editor_id": list(b"GSDocMitchellHouse"),
            "source_plugin": "FalloutNV.esm",
            "record_offset": 42,
        },
        "load_order": ["FalloutNV.esm"],
        "load_order_sha256": "a" * 64,
        "plugin_sha256": {"FalloutNV.esm": "b" * 64},
        "models": [
            {
                "path": "meshes/test.nif",
                "model_index": 0,
                "report": {
                    "retail_parity_accepted": False,
                    "model_sha256": "c" * 64,
                    "meshes": 1,
                    "vertices": 3,
                    "triangles": 1,
                    "textures": [
                        {
                            "path": "textures/test.dds",
                            "sha256": "d" * 64,
                            "width": 1,
                            "height": 1,
                            "mip_levels": 1,
                            "format": "BC1",
                        }
                    ],
                    "warnings": [],
                    "bindings": [],
                    "bounds": [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
                },
                "error": None,
            }
        ],
        "placements": [
            {
                "key": {"origin_plugin": "FalloutNV.esm", "local_id": 456},
                "model": 0,
                "source_affine": {"rows": [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 2.0], [0.0, 0.0, 1.0, 3.0]]},
                "status": "rendered-static-view",
            }
        ],
        "unique_render_models": 1,
        "rendered_references": 1,
        "rendered_mesh_instances": 1,
        "shared_geometry_vertices": 3,
        "shared_geometry_triangles": 1,
        "unique_texture_samplers": 1,
        "source_origin": [1.0, 2.0, 3.0],
        "coordinates": "source units; [x,y,z] -> [x,z,-y]",
        "rendering": "unlit static views; no retail lighting/effects or collision parity",
        "runtime_ready": False,
        "retail_parity_accepted": False,
        "source_residency": {
            "root": source_root,
            "generation": 2,
            "identity": "model-plan-identity",
            "texture_identity": "texture-plan-identity",
            "stage": "Decoded",
            "texture_state": "Decoded",
            "completed_models": 1,
            "requested_models": 1,
            "complete_model_coverage": True,
            "completed_textures": 1,
            "requested_textures": 1,
            "complete_texture_coverage": True,
            "texture_reference_coverage_verified": True,
            "pinned_source_bytes": 4096,
            "retained_plans": 1,
            "mapped_source_bytes": 1024,
            "dependencies": "Ready",
            "collision": "Unsupported",
            "behavior": "Unsupported",
            "simulation_ready": False,
            "render_published": False,
            "failure": None,
        },
        "camera_transform": {
            "scene_generation": 2,
            "source_root_sha256": scene_replay.digest_json(source_root),
            "camera_request_sha256": scene_replay.digest_json(camera_request),
            "world_from_camera": [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        },
        "scene_completion": {
            "state": "complete",
            "scene_generation": 2,
            "source_root_sha256": scene_replay.digest_json(source_root),
            "expected_references": 1,
            "rendered_references": 1,
            "resource_references": 1,
        },
        "canonical_state": None,
    }


def write_complete_manifest(
    root: Path, *, drop_camera_transform: bool = False, drop_scene_completion: bool = False
) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    report = sample_report()
    if drop_camera_transform:
        report.pop("camera_transform")
    if drop_scene_completion:
        report.pop("scene_completion")
    report_path = root / "view.json"
    image_path = root / "view.png"
    log_path = root / "view.log"
    report_path.write_bytes(scene_replay.canonical_json(report) + b"\n")
    pixels = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
    image_path.write_bytes(rgb_png(2, 2, pixels))
    log_path.write_bytes(b"")
    started_ns = min(path.stat().st_mtime_ns for path in (report_path, image_path, log_path)) - 1
    finished_ns = time.time_ns() + 1_000_000
    files = {
        "report": scene_replay._file_record(report_path, root, started_ns, finished_ns),
        "image": scene_replay._file_record(image_path, root, started_ns, finished_ns),
        "log": scene_replay._file_record(log_path, root, started_ns, finished_ns, allow_empty=True),
    }
    load_order_sha = "a" * 64
    evidence = scene_replay.scene_evidence(report, "GSDocMitchellHouse", load_order_sha)
    camera_request = {"source_position": [1.0, 2.0, 3.0], "source_target": [4.0, 5.0, 6.0]}
    image = {
        "width": 2,
        "height": 2,
        "sha256": scene_replay.sha256_file(image_path),
        "pixels_differing_from_first": scene_replay.pixels_differ_from_first(pixels),
    }
    scenario = {
        "id": "view",
        "required": True,
        "state": "completed",
        "interval": {"started_ns": started_ns, "finished_ns": finished_ns},
        "files": files,
        "report_sha256": scene_replay.sha256_file(report_path),
        "image": image,
        "image_tolerance": {"max_channel_delta": 0, "max_changed_pixels": 0},
        "camera_request": camera_request,
        "evidence": evidence,
        "observations": scene_replay.scene_observations(evidence, camera_request, image),
    }
    manifest = {
        "schema_version": scene_replay.SCHEMA_VERSION,
        "tool_version": scene_replay.TOOL_VERSION,
        "run_id": "complete-self-compare",
        "cell_editor_id": "GSDocMitchellHouse",
        "load_order_file": {"path": "load-order.txt", "sha256": load_order_sha},
        "source": {"tree_sha256": "b" * 64},
        "binary": {"path": "preview.exe", "sha256": "c" * 64, "bytes": 1},
        "config_sha256": "d" * 64,
        "scenarios": [scenario],
        "completion": {"state": "complete", "required": 1, "completed": 1, "failed": 0, "skipped": 0},
    }
    manifest_path = root / "manifest.json"
    manifest_path.write_bytes(scene_replay.canonical_json(manifest) + b"\n")
    return manifest_path


def source_fingerprint_workspace(root: Path) -> Path:
    files = {
        "Cargo.toml": '[workspace]\nmembers = ["crates/fallout-preview", "crates/fallout-data", "crates/fallout-runtime", "crates/fallout-math"]\n',
        "Cargo.lock": "version = 4\n",
        "rust-toolchain.toml": '[toolchain]\nchannel = "stable"\n',
        ".cargo/config.toml": "[build]\njobs = 1\n",
        "crates/fallout-preview/Cargo.toml": '[package]\nname = "fallout-preview"\nversion = "0.1.0"\nedition = "2024"\n[dependencies]\nfallout-data = { path = "../fallout-data" }\nfallout-runtime = { path = "../fallout-runtime" }\n',
        "crates/fallout-preview/src/main.rs": '#[path = "lib.rs"] mod engine_bridge;\n',
        "crates/fallout-preview/src/lib.rs": 'pub const INSPECTION_SHADER: &str = include_str!("inspection.wgsl");\n',
        "crates/fallout-preview/src/inspection.wgsl": "@fragment fn inspect() {}\n",
        "crates/fallout-data/Cargo.toml": '[package]\nname = "fallout-data"\nversion = "0.1.0"\nedition = "2024"\n[dependencies]\nfallout-math = { path = "../fallout-math" }\n',
        "crates/fallout-data/src/lib.rs": '#[path = "world/preparation/plan.rs"] mod preparation_plan;\n',
        "crates/fallout-data/src/world/preparation/plan.rs": "pub fn load_cell_model_plan() -> usize { 1 }\n",
        "crates/fallout-runtime/Cargo.toml": '[package]\nname = "fallout-runtime"\nversion = "0.1.0"\nedition = "2024"\n[dependencies]\nfallout-data = { path = "../fallout-data" }\n',
        "crates/fallout-runtime/src/lib.rs": "pub fn source_runtime() {}\n",
        "crates/fallout-math/Cargo.toml": '[package]\nname = "fallout-math"\nversion = "0.1.0"\nedition = "2024"\n',
        "crates/fallout-math/src/lib.rs": "pub fn source_math() {}\n",
        "tools/build-helper.rs": "fn main() {}\n",
    }
    for relative, contents in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
    return root


class SceneReportMutationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.expected = scene_replay.scene_evidence(sample_report(), "GSDocMitchellHouse", "a" * 64)

    def test_missing_texture_resource_fails_the_frozen_assertion(self) -> None:
        changed_report = sample_report()
        changed_report["models"][0]["report"]["textures"].clear()
        actual = scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "resources changed"):
            scene_replay.compare_scene_evidence(self.expected, actual)

    def test_changed_source_pose_fails_the_frozen_assertion(self) -> None:
        changed_report = sample_report()
        changed_report["placements"][0]["source_affine"]["rows"][0][3] += 0.25
        actual = scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "poses changed"):
            scene_replay.compare_scene_evidence(self.expected, actual)

    def test_missing_required_model_report_is_refused(self) -> None:
        changed_report = sample_report()
        changed_report["models"][0]["report"] = None
        changed_report["models"][0]["error"] = "missing model bytes"
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "Selected model resource"):
            scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

    def test_missing_expected_draw_or_resource_is_refused(self) -> None:
        for field, value in (("model", None), ("status", "initially-disabled")):
            with self.subTest(field=field):
                changed_report = sample_report()
                changed_report["placements"][0][field] = value
                with self.assertRaisesRegex(scene_replay.AcceptanceError, "Expected draw/resource is missing"):
                    scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

    def test_protected_435_reference_membership_cannot_pass_with_400_draws(self) -> None:
        changed_report = sample_report()
        template = changed_report["placements"][0]
        changed_report["placements"] = [
            {
                **template,
                "key": {"origin_plugin": "fixture.esm", "local_id": 10_000 + index},
            }
            for index in range(435)
        ]
        changed_report["rendered_references"] = 400
        with self.assertRaisesRegex(
            scene_replay.AcceptanceError,
            "435 protected placements but only 400 rendered references",
        ):
            scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

    def test_missing_reference_from_candidate_changes_protected_membership(self) -> None:
        expected_report = sample_report()
        template = expected_report["placements"][0]
        expected_report["placements"] = [
            {**template, "key": {"origin_plugin": "fixture.esm", "local_id": index}}
            for index in (10, 11)
        ]
        expected_report["rendered_references"] = 2
        expected = scene_replay.scene_evidence(expected_report, "GSDocMitchellHouse", "a" * 64)
        actual_report = sample_report()
        actual_report["placements"][0]["key"] = {"origin_plugin": "fixture.esm", "local_id": 10}
        actual = scene_replay.scene_evidence(actual_report, "GSDocMitchellHouse", "a" * 64)
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "reference_membership changed"):
            scene_replay.compare_scene_evidence(expected, actual)

    def test_source_root_and_generation_are_pinned_in_scene_evidence(self) -> None:
        changed_report = sample_report()
        changed_report["source_residency"]["root"]["local_id"] += 1
        changed_report["source_residency"]["generation"] += 1
        actual = scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "resources changed"):
            scene_replay.compare_scene_evidence(self.expected, actual)

    def test_empty_cell_capture_is_not_scene_evidence(self) -> None:
        changed_report = sample_report()
        changed_report["placements"] = []
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "no observed model placements"):
            scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

    def test_boolean_resource_count_is_not_an_integer_count(self) -> None:
        changed_report = sample_report()
        changed_report["source_residency"]["completed_models"] = True
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "completed models must be an integer"):
            scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

    def test_array_placement_model_index_is_a_structured_refusal(self) -> None:
        changed_report = sample_report()
        changed_report["placements"][0]["model"] = []
        with self.assertRaisesRegex(scene_replay.AcceptanceError, r"placements\[0\].model must be an integer"):
            scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

    def test_json_comparison_does_not_treat_boolean_as_integer(self) -> None:
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "Frozen scene cell changed"):
            scene_replay.compare_scene_evidence(
                {"cell": {"count": 1}}, {"cell": {"count": True}}
            )

    def test_zero_visible_render_count_is_not_scene_evidence(self) -> None:
        changed_report = sample_report()
        changed_report["rendered_mesh_instances"] = 0
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "no visible rendered mesh instances"):
            scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

    def test_source_fingerprint_covers_decode_and_render_inputs(self) -> None:
        repo = SCRIPT.parents[2]
        fingerprint = scene_replay.source_fingerprint(repo)
        paths = {row["path"] for row in fingerprint["files"]}
        for path in (
            "crates/fallout-preview/src/native.rs",
            "crates/fallout-preview/src/material.rs",
            "crates/fallout-preview/src/inspection.wgsl",
            "crates/fallout-data/src/nif_scene/material.rs",
            "crates/fallout-data/src/world/preparation/plan.rs",
            "crates/fallout-data/src/world/residency/textures.rs",
            "crates/fallout-runtime/src/lib.rs",
            "Cargo.lock",
            "crates/fallout-preview/Cargo.toml",
            "tools/team-build.py",
        ):
            self.assertIn(path, paths)
        self.assertEqual(fingerprint["source_roots"], ["crates", "tools"])
        self.assertEqual(fingerprint["source_file_count"], len(paths))
        self.assertGreater(fingerprint["source_total_bytes"], 0)
        self.assertEqual(
            [row["path"] for row in fingerprint["files"]],
            sorted(paths, key=lambda item: (item.casefold(), item)),
        )

    def test_source_fingerprint_detects_changed_transitive_and_new_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = source_fingerprint_workspace(Path(directory) / "repo")

            def fixed_git(_repo: Path, *_arguments: str) -> str:
                return "a" * 40 if _arguments[-1] == "HEAD" else "b" * 40

            with patch.object(scene_replay, "_git_value", side_effect=fixed_git):
                before = scene_replay.source_fingerprint(repo)
                self.assertIn(
                    "crates/fallout-math/src/lib.rs",
                    {row["path"] for row in before["files"]},
                )
                plan_path = repo / "crates/fallout-data/src/world/preparation/plan.rs"
                plan_path.write_text("pub fn load_cell_model_plan() -> usize { 2 }\n", encoding="utf-8")
                changed = scene_replay.source_fingerprint(repo)
                self.assertEqual(before["git_tree"], changed["git_tree"])
                before_plan = next(row for row in before["files"] if row["path"].endswith("/world/preparation/plan.rs"))
                changed_plan = next(row for row in changed["files"] if row["path"] == before_plan["path"])
                self.assertNotEqual(before_plan["sha256"], changed_plan["sha256"])
                self.assertNotEqual(before["files"], changed["files"])

                new_module = repo / "crates/fallout-preview/src/engine_bridge/upload.rs"
                new_module.parent.mkdir(parents=True, exist_ok=True)
                new_module.write_text("pub fn upload_observer() {}\n", encoding="utf-8")
                extended = scene_replay.source_fingerprint(repo)
                self.assertIn(
                    "crates/fallout-preview/src/engine_bridge/upload.rs",
                    {row["path"] for row in extended["files"]},
                )
                self.assertNotEqual(changed["files"], extended["files"])

    def test_source_fingerprint_detects_changed_nested_local_module(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = source_fingerprint_workspace(Path(directory) / "repo")
            module = repo / "crates/fallout-data/src/local/mod.rs"
            module.parent.mkdir(parents=True)
            module.write_text("pub fn local_source() -> u32 { 1 }\n", encoding="utf-8")
            (repo / "crates/fallout-data/src/lib.rs").write_text("pub mod local;\n", encoding="utf-8")
            private_evidence = repo / "local/v4-evidence/retail.json"
            private_evidence.parent.mkdir(parents=True)
            private_evidence.write_text('{"private":true}\n', encoding="utf-8")

            def fixed_git(_repo: Path, *_arguments: str) -> str:
                return "a" * 40 if _arguments[-1] == "HEAD" else "b" * 40

            with patch.object(scene_replay, "_git_value", side_effect=fixed_git):
                before = scene_replay.source_fingerprint(repo)
                before_paths = {row["path"] for row in before["files"]}
                self.assertIn("crates/fallout-data/src/local/mod.rs", before_paths)
                self.assertNotIn("local/v4-evidence/retail.json", before_paths)

                module.write_text("pub fn local_source() -> u32 { 2 }\n", encoding="utf-8")
                changed = scene_replay.source_fingerprint(repo)

            self.assertEqual(before["git_tree"], changed["git_tree"])
            before_module = next(
                row
                for row in before["files"]
                if row["path"] == "crates/fallout-data/src/local/mod.rs"
            )
            changed_module = next(
                row
                for row in changed["files"]
                if row["path"] == "crates/fallout-data/src/local/mod.rs"
            )
            self.assertNotEqual(before_module["sha256"], changed_module["sha256"])
            self.assertNotEqual(before["files"], changed["files"])

    def test_source_fingerprint_refuses_source_references_outside_roots(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = source_fingerprint_workspace(Path(directory) / "repo")
            outside = Path(directory) / "outside.rs"
            outside.write_text("pub fn escape() {}\n", encoding="utf-8")
            source = repo / "crates/fallout-preview/src/main.rs"
            relative = os.path.relpath(outside, source.parent).replace("\\", "/")
            source.write_text(f'#[path = "{relative}"] mod outside;\n', encoding="utf-8")
            with patch.object(scene_replay, "_git_value", return_value="a" * 40):
                with self.assertRaisesRegex(scene_replay.AcceptanceError, "escapes the pinned source roots"):
                    scene_replay.source_fingerprint(repo)

    def test_source_fingerprint_refuses_cargo_path_dependencies_outside_roots(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = source_fingerprint_workspace(Path(directory) / "repo")
            outside = Path(directory) / "outside-package"
            outside.mkdir()
            (outside / "Cargo.toml").write_text(
                '[package]\nname = "outside-package"\nversion = "0.1.0"\nedition = "2024"\n',
                encoding="utf-8",
            )
            manifest = repo / "crates/fallout-preview/Cargo.toml"
            manifest.write_text(
                manifest.read_text(encoding="utf-8")
                + 'fallout-unlisted = { path = "../../../outside-package" }\n',
                encoding="utf-8",
            )
            with patch.object(scene_replay, "_git_value", return_value="a" * 40):
                with self.assertRaisesRegex(scene_replay.AcceptanceError, "Cargo source path escapes"):
                    scene_replay.source_fingerprint(repo)

    def test_source_fingerprint_enforces_inventory_budgets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = source_fingerprint_workspace(Path(directory) / "repo")
            with patch.object(scene_replay, "_git_value", return_value="a" * 40):
                with patch.object(scene_replay, "MAX_SOURCE_FILES", 3):
                    with self.assertRaisesRegex(scene_replay.AcceptanceError, "file inventory budget"):
                        scene_replay.source_fingerprint(repo)
                with patch.object(scene_replay, "MAX_SOURCE_BYTES", 8):
                    with self.assertRaisesRegex(scene_replay.AcceptanceError, "byte inventory budget"):
                        scene_replay.source_fingerprint(repo)


class CompletionAndObservationTests(unittest.TestCase):
    def test_all_skipped_required_runs_are_not_a_pass(self) -> None:
        manifest = {
            "scenarios": [
                {"id": "one", "required": True, "state": "skipped"},
                {"id": "two", "required": True, "state": "skipped"},
            ],
            "completion": {"state": "incomplete", "required": 2, "completed": 0, "failed": 0, "skipped": 2},
        }
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "all-skipped"):
            scene_replay.validate_completion(manifest)

    def test_incomplete_per_run_matrix_stays_incomplete(self) -> None:
        manifest = {
            "scenarios": [
                {"id": "one", "required": True, "state": "completed"},
                {"id": "two", "required": True, "state": "failed"},
            ],
            "completion": {"state": "incomplete", "required": 2, "completed": 1, "failed": 1, "skipped": 0},
        }
        completion = scene_replay.validate_completion(manifest)
        self.assertEqual(completion["state"], "incomplete")
        self.assertEqual(completion["completed"], 1)
        self.assertEqual(completion["failed"], 1)

    def test_boolean_manifest_schema_is_not_schema_version_one(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            with self.assertRaisesRegex(scene_replay.AcceptanceError, "manifest.schema_version must be an integer"):
                scene_replay.validate_manifest_files(path, {"schema_version": True})

    def test_boolean_scenario_config_schema_is_not_schema_version_one(self) -> None:
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "scenario config.schema_version must be an integer"):
            scene_replay.validate_capture_config({"schema_version": True})

    def test_boolean_completion_count_is_not_an_integer_count(self) -> None:
        manifest = {
            "scenarios": [{"id": "one", "required": True, "state": "completed"}],
            "completion": {"required": 1, "completed": True, "failed": 0, "skipped": 0, "state": "complete"},
        }
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "completion.completed must be an integer"):
            scene_replay.validate_completion(manifest)

    def test_absent_aggregate_completion_is_refused(self) -> None:
        manifest = {"scenarios": [{"id": "one", "required": True, "state": "completed"}]}
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "no aggregate completion observation"):
            scene_replay.validate_completion(manifest)

    def test_absent_observations_are_not_reported_as_passes(self) -> None:
        report = sample_report()
        report.pop("camera_transform")
        report.pop("scene_completion")
        evidence = scene_replay.scene_evidence(report, "GSDocMitchellHouse", "a" * 64)
        observations = scene_replay.scene_observations(
            evidence,
            {"source_position": [1.0, 2.0, 3.0], "source_target": [4.0, 5.0, 6.0]},
            {"width": 1, "height": 1, "sha256": "e" * 64},
        )
        self.assertEqual(observations["camera_transform"]["status"], "absent")
        self.assertEqual(observations["scene_completion"]["status"], "absent")
        self.assertEqual(observations["resource_drain"]["status"], "absent")
        self.assertEqual(observations["simulation_readiness"]["status"], "unsupported")
        for name in ("camera_transform", "scene_completion", "resource_drain", "simulation_readiness"):
            self.assertNotEqual(observations[name]["status"], "pass")

    def test_stale_camera_generation_and_mismatched_completion_are_refused(self) -> None:
        report = sample_report()
        report["camera_transform"]["scene_generation"] = 1
        report["scene_completion"]["rendered_references"] = 0
        evidence = scene_replay.scene_evidence(report, "GSDocMitchellHouse", "a" * 64)
        observations = scene_replay.scene_observations(
            evidence,
            {"source_position": [1.0, 2.0, 3.0], "source_target": [4.0, 5.0, 6.0]},
            {"width": 1, "height": 1, "sha256": "e" * 64},
        )
        self.assertEqual(observations["camera_transform"]["status"], "mismatch")
        self.assertEqual(observations["scene_completion"]["status"], "mismatch")

    def test_duplicate_scenario_ids_are_refused(self) -> None:
        config = {
            "schema_version": 1,
            "cell_editor_id": "cell",
            "runs": [
                {"id": "same", "camera": {"source_position": [0, 0, 0], "source_target": [0, 1, 0]}},
                {"id": "same", "camera": {"source_position": [0, 0, 0], "source_target": [1, 0, 0]}},
            ],
        }
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "Duplicate scenario"):
            scene_replay.validate_capture_config(config)

    def test_scenario_id_cannot_escape_capture_directory(self) -> None:
        config = {
            "schema_version": 1,
            "cell_editor_id": "cell",
            "runs": [
                {
                    "id": "..\\outside",
                    "camera": {"source_position": [0, 0, 0], "source_target": [0, 1, 0]},
                }
            ],
        }
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "safe filename component"):
            scene_replay.validate_capture_config(config)


class ImageAndFreshnessTests(unittest.TestCase):
    def test_complete_manifest_compares_against_itself_with_empty_log(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = write_complete_manifest(Path(directory))
            result = scene_replay.compare_capture_manifests(manifest, manifest)
            self.assertEqual(result["harness_status"], "pass", result["failures"])
            self.assertEqual(result["scenarios"][0]["status"], "pass")

    def test_absent_camera_or_scene_completion_cannot_pass_as_partial(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            expected = write_complete_manifest(root / "expected")
            actual = write_complete_manifest(
                root / "actual", drop_camera_transform=True, drop_scene_completion=True
            )
            result = scene_replay.compare_capture_manifests(expected, actual)
            self.assertEqual(result["harness_status"], "fail")
            self.assertTrue(any("camera_transform observation is absent" in row for row in result["failures"]))
            self.assertTrue(any("scene_completion observation is absent" in row for row in result["failures"]))

    def test_json_size_budget_is_checked_before_read(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "oversized.json"
            path.write_bytes(b"{}")
            with patch.object(scene_replay, "MAX_JSON_BYTES", 1):
                with self.assertRaisesRegex(scene_replay.AcceptanceError, "exceeds the 1-byte input budget"):
                    scene_replay.read_json(path)

    def test_failed_run_keeps_fresh_artifact_receipts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            started_ns = time.time_ns() - 1_000_000_000
            image_path = root / "view.png"
            report_path = root / "view.json"
            log_path = root / "view.log"
            pixels = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
            image_path.write_bytes(rgb_png(2, 2, pixels))
            report_path.write_text('{"runtime_ready":false}\n', encoding="utf-8")
            log_path.write_bytes(b"")
            finished_ns = time.time_ns() + 1_000_000_000
            row = {"state": "failed", "error": "Cell model residency is incomplete"}
            scene_replay.attach_failed_artifacts(
                row,
                root,
                {"image": image_path, "report": report_path, "log": log_path},
                started_ns,
                finished_ns,
            )
            self.assertEqual(row["state"], "failed")
            self.assertEqual(row["error"], "Cell model residency is incomplete")
            self.assertEqual(set(row["files"]), {"image", "report", "log"}, row.get("artifact_errors"))
            self.assertEqual(row["report_sha256"], scene_replay.sha256_file(report_path))
            self.assertEqual(row["image"]["width"], 2)

    def test_verification_result_must_stay_in_private_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory) / "repo"
            private = repo / "local" / "v4-evidence"
            private.mkdir(parents=True)
            self.assertEqual(
                scene_replay._check_private_evidence_path(repo, private / "result.json"),
                private / "result.json",
            )
            with self.assertRaisesRegex(scene_replay.AcceptanceError, "local/v4-evidence"):
                scene_replay._check_private_evidence_path(repo, Path(directory) / "outside.json")

    def test_exact_image_match_and_declared_pixel_tolerance(self) -> None:
        first = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
        changed = bytes([255, 0, 0, 0, 255, 0, 0, 0, 250, 255, 255, 255])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            expected = root / "expected.png"
            actual = root / "actual.png"
            expected.write_bytes(rgb_png(2, 2, first))
            actual.write_bytes(rgb_png(2, 2, first))
            exact = scene_replay.compare_png(
                expected, actual, {"max_channel_delta": 0, "max_changed_pixels": 0}
            )
            self.assertEqual(exact["changed_pixels"], 0)
            actual.write_bytes(rgb_png(2, 2, changed))
            tolerated = scene_replay.compare_png(
                expected, actual, {"max_channel_delta": 5, "max_changed_pixels": 1}
            )
            self.assertEqual(tolerated["changed_pixels"], 1)
            with self.assertRaisesRegex(scene_replay.AcceptanceError, "maximum channel delta"):
                scene_replay.compare_png(
                    expected, actual, {"max_channel_delta": 4, "max_changed_pixels": 1}
                )

    def test_blank_capture_is_refused(self) -> None:
        pixels = bytes([12, 12, 12] * 4)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "blank.png"
            path.write_bytes(rgb_png(2, 2, pixels))
            with self.assertRaisesRegex(scene_replay.AcceptanceError, "blank"):
                scene_replay.compare_png(
                    path, path, {"max_channel_delta": 0, "max_changed_pixels": 0}
                )

    def test_stale_output_timestamp_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "old.png"
            path.write_bytes(b"old")
            old_ns = 1_000_000_000
            os.utime(path, ns=(old_ns, old_ns))
            record = {
                "path": "old.png",
                "bytes": 3,
                "sha256": scene_replay.sha256_file(path),
                "mtime_ns": old_ns,
            }
            with self.assertRaisesRegex(scene_replay.AcceptanceError, "stale"):
                scene_replay.validate_file_record(root, record, old_ns + 1, old_ns + 100, "image")


if __name__ == "__main__":
    unittest.main()
