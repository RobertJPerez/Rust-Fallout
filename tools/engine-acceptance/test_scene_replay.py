from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import struct
import tempfile
import unittest
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
        "canonical_state": None,
    }


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

    def test_empty_cell_capture_is_not_scene_evidence(self) -> None:
        changed_report = sample_report()
        changed_report["placements"] = []
        with self.assertRaisesRegex(scene_replay.AcceptanceError, "no observed model placements"):
            scene_replay.scene_evidence(changed_report, "GSDocMitchellHouse", "a" * 64)

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
            "crates/fallout-data/src/nif_scene/material.rs",
            "crates/fallout-data/src/world/residency/textures.rs",
        ):
            self.assertIn(path, paths)


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

    def test_absent_observations_are_not_reported_as_passes(self) -> None:
        evidence = scene_replay.scene_evidence(sample_report(), "GSDocMitchellHouse", "a" * 64)
        observations = scene_replay.scene_observations(
            evidence,
            {"source_position": [1.0, 2.0, 3.0], "source_target": [4.0, 5.0, 6.0]},
            {"width": 1, "height": 1, "sha256": "e" * 64},
        )
        self.assertEqual(observations["camera_transform"]["status"], "absent")
        self.assertEqual(observations["resource_drain"]["status"], "absent")
        self.assertEqual(observations["simulation_readiness"]["status"], "unsupported")
        for name in ("camera_transform", "resource_drain", "simulation_readiness"):
            self.assertNotEqual(observations[name]["status"], "pass")

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
