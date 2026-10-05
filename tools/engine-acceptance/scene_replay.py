"""Capture and compare frozen, source-bound scene preview evidence.

This is an engineering replay harness. A successful comparison means that two
captures agree with a separately frozen engineering reference; it does not
establish retail-game parity or simulation readiness.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time
import tomllib
import uuid
import zlib


SCHEMA_VERSION = 1
TOOL_VERSION = "scene-replay-4"
MAX_JSON_BYTES = 64 * 1024 * 1024
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
SOURCE_ROOTS = ("crates", "tools")
# Inventory a conservative superset so new modules, local crates and shaders
# cannot escape a hand-maintained transitive-source list.
SOURCE_CONTROL_FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")
SOURCE_OPTIONAL_ROOTS = (".cargo",)
SOURCE_IGNORED_DIRS = frozenset(
    {".git", "target", "target-v4", "__pycache__", ".pytest_cache"}
)
# Workspace-level local evidence is outside SOURCE_ROOTS. Do not exclude every
# directory named "local": src/local/mod.rs is ordinary production source.
MAX_SOURCE_FILES = 4096
MAX_SOURCE_FILE_BYTES = 64 * 1024 * 1024
MAX_SOURCE_BYTES = 256 * 1024 * 1024
MAX_SOURCE_PATH_CHARS = 1024
RUST_PATH_ATTRIBUTE_CALL = re.compile(r"#\s*\[\s*path\b")
RUST_PATH_ATTRIBUTE = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]')
RUST_INCLUDE_CALL = re.compile(r"\binclude(?:_str|_bytes)?\s*!\s*\(")
RUST_INCLUDE_LITERAL = re.compile(
    r'\binclude(?:_str|_bytes)?\s*!\s*\(\s*"([^"]+)"\s*\)'
)
ABSENT_OBSERVATIONS = {
    "actual_camera_transform": "The preview report records camera inputs, not the applied Transform.",
    "canonical_player_pose": "This static-cell replay has no body-motion consumer or canonical player pose receipt.",
    "resource_drain": "The preview report is written before GPU admission and does not report post-exit lease drain.",
    "frame_readiness_receipt": "The capture has a PNG, but the host does not emit a per-frame readiness receipt.",
}


class AcceptanceError(ValueError):
    """Invalid, incomplete, stale, or mismatched acceptance evidence."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def canonical_json(value: object) -> bytes:
    try:
        return json.dumps(
            value, ensure_ascii=False, allow_nan=False, sort_keys=True, separators=(",", ":")
        ).encode("utf-8")
    except (TypeError, ValueError) as error:
        raise AcceptanceError(f"Value cannot be represented as finite canonical JSON: {error}") from error


def digest_json(value: object) -> str:
    return sha256_bytes(canonical_json(value))


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise AcceptanceError(f"Duplicate JSON object key: {key}")
        result[key] = value
    return result


def read_json(path: Path) -> object:
    try:
        if path.stat().st_size > MAX_JSON_BYTES:
            raise AcceptanceError(f"JSON file exceeds the {MAX_JSON_BYTES}-byte input budget: {path}")
        with path.open("rb") as stream:
            data = stream.read(MAX_JSON_BYTES + 1)
        if len(data) > MAX_JSON_BYTES:
            raise AcceptanceError(f"JSON file exceeds the {MAX_JSON_BYTES}-byte input budget: {path}")
        return json.loads(
            data.decode("utf-8"),
            object_pairs_hook=_unique_object,
            parse_constant=lambda value: (_ for _ in ()).throw(
                AcceptanceError(f"Non-finite JSON number: {value}")
            ),
        )
    except AcceptanceError:
        raise
    except (OSError, UnicodeError, ValueError) as error:
        raise AcceptanceError(f"Cannot read JSON {path}: {error}") from error


def write_json_new(path: Path, value: object) -> None:
    """Create evidence exclusively. Existing files are never replaced."""
    payload = json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    descriptor = os.open(path, flags, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
    except BaseException:
        try:
            os.close(descriptor)
        except OSError:
            pass
        raise


def _object(value: object, name: str) -> dict:
    if not isinstance(value, dict):
        raise AcceptanceError(f"{name} must be a JSON object")
    return value


def _string(value: object, name: str) -> str:
    if not isinstance(value, str) or not value:
        raise AcceptanceError(f"{name} must be a non-empty string")
    return value


def _integer(
    value: object, name: str, minimum: int | None = None, maximum: int | None = None
) -> int:
    if type(value) is not int:
        raise AcceptanceError(f"{name} must be an integer")
    if minimum is not None and value < minimum:
        raise AcceptanceError(f"{name} must be at least {minimum}")
    if maximum is not None and value > maximum:
        raise AcceptanceError(f"{name} must be at most {maximum}")
    return value


def _strict_json_equal(left: object, right: object) -> bool:
    """Compare JSON values without Python's bool/int or int/float coercions."""
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(
            _strict_json_equal(left[key], right[key]) for key in left
        )
    if isinstance(left, list):
        return len(left) == len(right) and all(
            _strict_json_equal(a, b) for a, b in zip(left, right)
        )
    return left == right


def _sha256(value: object, name: str) -> str:
    if not isinstance(value, str) or not SHA256_RE.fullmatch(value):
        raise AcceptanceError(f"{name} must be a lowercase SHA-256 digest")
    return value


def _finite_vector(value: object, name: str, length: int = 3) -> list[float]:
    if not isinstance(value, list) or len(value) != length:
        raise AcceptanceError(f"{name} must contain {length} numbers")
    if any(isinstance(item, bool) or not isinstance(item, (int, float)) or not math.isfinite(item) for item in value):
        raise AcceptanceError(f"{name} must contain only finite numbers")
    return [float(item) for item in value]


def _safe_child(root: Path, relative: object, name: str) -> Path:
    if not isinstance(relative, str) or not relative:
        raise AcceptanceError(f"{name} path must be non-empty")
    candidate = Path(relative)
    if candidate.is_absolute() or any(part in ("..", "") for part in candidate.parts):
        raise AcceptanceError(f"{name} path must be relative and cannot escape its run directory")
    root_resolved = root.resolve()
    resolved = (root_resolved / candidate).resolve()
    if root_resolved != resolved and root_resolved not in resolved.parents:
        raise AcceptanceError(f"{name} path escapes its run directory")
    return resolved


def _file_record(
    path: Path, root: Path, started_ns: int, finished_ns: int, allow_empty: bool = False
) -> dict:
    stat = path.stat()
    if stat.st_size <= 0 and not allow_empty:
        raise AcceptanceError(f"Capture output is empty: {path.name}")
    if stat.st_mtime_ns < started_ns or stat.st_mtime_ns > finished_ns:
        raise AcceptanceError(f"Stale or out-of-run capture output: {path.name}")
    return {
        "path": path.resolve().relative_to(root.resolve()).as_posix(),
        "bytes": stat.st_size,
        "sha256": sha256_file(path),
        "mtime_ns": stat.st_mtime_ns,
    }


def attach_failed_artifacts(
    row: dict, output: Path, files: dict[str, Path], started_ns: int, finished_ns: int
) -> None:
    """Pin fresh outputs from a failed run without changing its failed state."""
    records = {}
    errors = []
    for kind, path in files.items():
        if not path.is_file():
            continue
        try:
            records[kind] = _file_record(
                path,
                output,
                started_ns,
                finished_ns,
                allow_empty=kind == "log",
            )
        except (OSError, AcceptanceError) as error:
            errors.append(f"{kind}: {error}")
    if records:
        row["files"] = records
    if "report" in records:
        row["report_sha256"] = records["report"]["sha256"]
    if "image" in records:
        try:
            width, height, pixels = read_rgb_png(files["image"])
            row["image"] = {
                "width": width,
                "height": height,
                "sha256": records["image"]["sha256"],
                "pixels_differing_from_first": pixels_differ_from_first(pixels),
            }
        except (OSError, AcceptanceError) as error:
            errors.append(f"image decode: {error}")
    if errors:
        row["artifact_errors"] = errors


def validate_file_record(
    root: Path,
    record: object,
    started_ns: int,
    finished_ns: int,
    name: str,
    allow_empty: bool = False,
) -> Path:
    row = _object(record, name)
    path = _safe_child(root, row.get("path"), name)
    if not path.is_file():
        raise AcceptanceError(f"{name} is missing: {row.get('path')}")
    stat = path.stat()
    recorded_bytes = _integer(row.get("bytes"), f"{name}.bytes", minimum=0 if allow_empty else 1)
    if stat.st_size != recorded_bytes:
        raise AcceptanceError(f"{name} length changed or is empty")
    recorded_mtime = _integer(row.get("mtime_ns"), f"{name}.mtime_ns", minimum=1)
    if stat.st_mtime_ns != recorded_mtime:
        raise AcceptanceError(f"{name} timestamp changed after capture")
    if stat.st_mtime_ns < started_ns or stat.st_mtime_ns > finished_ns:
        raise AcceptanceError(f"{name} is stale or outside its run interval")
    if sha256_file(path) != _sha256(row.get("sha256"), f"{name}.sha256"):
        raise AcceptanceError(f"{name} digest does not match its receipt")
    return path


def validate_capture_config(config: object) -> list[dict]:
    doc = _object(config, "scenario config")
    if _integer(doc.get("schema_version"), "scenario config.schema_version") != 1:
        raise AcceptanceError("scenario config schema_version must be 1")
    _string(doc.get("cell_editor_id"), "cell_editor_id")
    viewport = doc.get("viewport", {"width": 1280, "height": 900})
    viewport = _object(viewport, "viewport")
    width = _integer(viewport.get("width"), "viewport.width")
    height = _integer(viewport.get("height"), "viewport.height")
    if width != 1280 or height != 900:
        raise AcceptanceError("This preview host captures cells at exactly 1280x900")
    runs = doc.get("runs")
    if not isinstance(runs, list) or not runs:
        raise AcceptanceError("scenario config requires at least one run")
    ids: set[str] = set()
    normalized = []
    for index, raw in enumerate(runs):
        row = _object(raw, f"runs[{index}]")
        run_id = _string(row.get("id"), f"runs[{index}].id")
        if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", run_id) is None:
            raise AcceptanceError(f"{run_id}: scenario id must be a safe filename component")
        if run_id in ids:
            raise AcceptanceError(f"Duplicate scenario id: {run_id}")
        ids.add(run_id)
        camera = _object(row.get("camera"), f"runs[{index}].camera")
        position = _finite_vector(camera.get("source_position"), "camera.source_position")
        target = _finite_vector(camera.get("source_target"), "camera.source_target")
        if position == target:
            raise AcceptanceError(f"{run_id}: camera position and target must differ")
        tolerance = _object(row.get("image_tolerance", {}), f"runs[{index}].image_tolerance")
        max_delta = _integer(tolerance.get("max_channel_delta", 0), f"{run_id}.max_channel_delta", 0, 255)
        max_pixels = _integer(tolerance.get("max_changed_pixels", 0), f"{run_id}.max_changed_pixels", 0)
        required = row.get("required", True)
        if not isinstance(required, bool):
            raise AcceptanceError(f"{run_id}: required must be boolean")
        normalized.append({
            "id": run_id,
            "required": required,
            "camera": {"source_position": position, "source_target": target},
            "image_tolerance": {"max_channel_delta": max_delta, "max_changed_pixels": max_pixels},
        })
    if not any(row["required"] for row in normalized):
        raise AcceptanceError("At least one scenario must be required")
    return normalized


def _decode_editor_id(value: object) -> str:
    if isinstance(value, list) and all(isinstance(item, int) and 0 <= item <= 255 for item in value):
        try:
            return bytes(value).decode("ascii")
        except UnicodeDecodeError as error:
            raise AcceptanceError("cell editor id is not ASCII") from error
    if isinstance(value, str):
        return value
    raise AcceptanceError("cell report has no decodable editor id")


def scene_evidence(report_value: object, expected_cell: str, expected_load_order_sha256: str) -> dict:
    report = _object(report_value, "scene report")
    if report.get("schema_version") not in (3, 4):
        raise AcceptanceError(f"Unsupported cell report schema: {report.get('schema_version')}")
    if report.get("retail_parity_accepted") is not False:
        raise AcceptanceError("Preview report must not claim retail parity")
    cell = _object(report.get("cell"), "cell report.cell")
    if _decode_editor_id(cell.get("editor_id")) != expected_cell:
        raise AcceptanceError("Captured cell differs from the requested cell")
    load_order_sha = _sha256(report.get("load_order_sha256"), "load_order_sha256")
    if load_order_sha != expected_load_order_sha256:
        raise AcceptanceError("Captured report does not match the supplied load-order file")
    names = report.get("load_order")
    if not isinstance(names, list) or not names or not all(isinstance(item, str) and item for item in names):
        raise AcceptanceError("Cell report must retain its complete ordered plugin list")
    plugin_hashes = _object(report.get("plugin_sha256"), "plugin_sha256")
    if set(plugin_hashes) != set(names):
        raise AcceptanceError("Plugin hashes do not cover the complete ordered load order")
    for name, digest in plugin_hashes.items():
        _sha256(digest, f"plugin_sha256[{name}]")

    snapshot = _object(report.get("source_residency"), "source_residency")
    source_root = _object(snapshot.get("root"), "source_residency.root")
    _string(source_root.get("profile"), "source_residency.root.profile")
    _string(source_root.get("origin_plugin"), "source_residency.root.origin_plugin")
    _integer(source_root.get("local_id"), "source_residency.root.local_id", 0)
    scene_generation = _integer(
        snapshot.get("generation"), "source_residency.generation", 1
    )
    model_identity = _string(snapshot.get("identity"), "source_residency.identity")
    texture_identity = _string(snapshot.get("texture_identity"), "source_residency.texture_identity")
    model_rows = report.get("models")
    placements = report.get("placements")
    if not isinstance(model_rows, list) or not isinstance(placements, list):
        raise AcceptanceError("Cell report must retain all model inspections and placements")
    if not model_rows or not placements:
        raise AcceptanceError("Selected cell report contains no observed model placements")
    rendered_references = _integer(report.get("rendered_references"), "rendered_references", 1)
    if rendered_references != len(placements):
        raise AcceptanceError(
            "Expected reference membership is incomplete: "
            f"{len(placements)} protected placements but only {rendered_references} rendered references"
        )
    for field in ("rendered_mesh_instances",):
        count = report.get(field)
        if isinstance(count, bool) or not isinstance(count, int) or count <= 0:
            raise AcceptanceError(f"Selected cell capture has no visible {field.replace('_', ' ')}")
    if snapshot.get("complete_model_coverage") is not True:
        raise AcceptanceError("Cell model residency is incomplete")
    if snapshot.get("complete_texture_coverage") is not True:
        raise AcceptanceError("Cell texture residency is incomplete")
    if snapshot.get("texture_reference_coverage_verified") is not True:
        raise AcceptanceError("Texture references were not fully verified")
    for completed, requested, label in (
        (snapshot.get("completed_models"), snapshot.get("requested_models"), "models"),
        (snapshot.get("completed_textures"), snapshot.get("requested_textures"), "textures"),
    ):
        completed = _integer(completed, f"completed {label}", 0)
        requested = _integer(requested, f"requested {label}", 0)
        if completed != requested:
            raise AcceptanceError(f"Cell {label} did not complete every requested resource job")

    models = []
    model_by_index = {}
    for index, raw in enumerate(model_rows):
        inspection = _object(raw, f"models[{index}]")
        model_report = inspection.get("report")
        row = {
            "path": inspection.get("path"),
            "model_index": inspection.get("model_index"),
            "error": inspection.get("error"),
            "status": "observed" if isinstance(model_report, dict) else (
                "missing" if inspection.get("error") else "absent"
            ),
        }
        if isinstance(model_report, dict):
            if model_report.get("retail_parity_accepted") is not False:
                raise AcceptanceError("Model report must not claim retail parity")
            model_hash = _sha256(model_report.get("model_sha256"), "model_sha256")
            texture_rows = model_report.get("textures")
            if not isinstance(texture_rows, list):
                raise AcceptanceError("Model report has no texture observation list")
            textures = []
            for texture_index, texture_value in enumerate(texture_rows):
                texture = _object(texture_value, f"model texture[{texture_index}]")
                textures.append({
                    "path": texture.get("path"),
                    "sha256": _sha256(texture.get("sha256"), "texture.sha256"),
                    "width": texture.get("width"),
                    "height": texture.get("height"),
                    "mip_levels": texture.get("mip_levels"),
                    "format": texture.get("format"),
                })
            row.update(
                model_sha256=model_hash,
                meshes=model_report.get("meshes"),
                vertices=model_report.get("vertices"),
                triangles=model_report.get("triangles"),
                textures=textures,
                warnings=model_report.get("warnings"),
                bindings=model_report.get("bindings"),
                bounds=model_report.get("bounds"),
            )
        if row["status"] != "observed":
            raise AcceptanceError(
                f"Selected model resource was not observed: {row['path']}: {row['error'] or 'no report'}"
            )
        model_index = _integer(row["model_index"], f"models[{index}].model_index", 0)
        if model_index in model_by_index:
            raise AcceptanceError(f"Invalid or duplicate model index: {model_index}")
        model_by_index[model_index] = row
        models.append(row)

    pose_rows = []
    placement_rows = []
    seen_reference_keys = set()
    for index, raw in enumerate(placements):
        placement = _object(raw, f"placements[{index}]")
        key = _object(placement.get("key"), f"placements[{index}].key")
        reference_key = canonical_json(key)
        if reference_key in seen_reference_keys:
            raise AcceptanceError(f"Duplicate protected reference key at placements[{index}]")
        seen_reference_keys.add(reference_key)
        model_index = placement.get("model")
        if model_index is None:
            raise AcceptanceError(f"Expected draw/resource is missing for placement {index}")
        model_index = _integer(model_index, f"placements[{index}].model", 0)
        if model_index not in model_by_index:
            raise AcceptanceError(f"Placement references missing model index {model_index}")
        if placement.get("status") != "rendered-static-view":
            raise AcceptanceError(f"Expected draw/resource is missing for placement {index}")
        pose_rows.append({
            "key": key,
            "source_affine": placement.get("source_affine"),
            "status": placement.get("status"),
        })
        placement_rows.append({
            "key": key,
            "model": model_index,
            "status": placement.get("status"),
        })
    pose_rows.sort(key=lambda row: canonical_json(row["key"]))
    placement_rows.sort(key=lambda row: canonical_json(row["key"]))

    canonical_state = report.get("canonical_state")
    canonical_bindings = None
    if canonical_state is not None:
        canonical = _object(canonical_state, "canonical_state")
        bindings = canonical.get("bindings")
        if not isinstance(bindings, list):
            raise AcceptanceError("Canonical observation is missing its bindings")
        canonical_bindings = [
            {
                "key": binding.get("key"),
                "source_affine": binding.get("source_affine"),
                "display": binding.get("display"),
                "canonical": binding.get("canonical"),
            }
            for binding in map(lambda item: _object(item, "canonical binding"), bindings)
        ]
        canonical_bindings.sort(key=lambda row: canonical_json(row["key"]))

    residency = {
        "root": source_root,
        "generation": scene_generation,
        **{
            key: snapshot.get(key)
            for key in (
            "stage", "texture_state", "identity", "texture_identity",
            "completed_models", "requested_models", "complete_model_coverage",
            "completed_textures", "requested_textures", "complete_texture_coverage",
            "texture_reference_coverage_verified", "pinned_source_bytes",
            "retained_plans", "mapped_source_bytes", "dependencies", "collision",
            "behavior", "simulation_ready", "render_published", "failure",
            )
        },
    }
    resources = {
        "residency": residency,
        "models": models,
        "placements": placement_rows,
    }
    reference_keys = [row["key"] for row in placement_rows]
    reference_keys.sort(key=canonical_json)
    camera_transform = report.get("camera_transform")
    if camera_transform is not None:
        camera_transform = _object(camera_transform, "camera_transform")
    completion_receipt = report.get("scene_completion")
    if completion_receipt is not None:
        completion_receipt = _object(completion_receipt, "scene_completion")
    poses = {"placements": pose_rows, "canonical_bindings": canonical_bindings}
    return {
        "cell": {
            "key": cell.get("key"),
            "source_plugin": cell.get("source_plugin"),
            "record_offset": cell.get("record_offset"),
        },
        "inputs": {
            "load_order_sha256": load_order_sha,
            "load_order": names,
            "plugin_sha256": dict(sorted(plugin_hashes.items())),
        },
        "camera_origin": report.get("source_origin"),
        "coordinates": report.get("coordinates"),
        "scene_report": {
            "schema_version": report.get("schema_version"),
            "rendering": report.get("rendering"),
            "runtime_ready": report.get("runtime_ready"),
            "retail_parity_accepted": report.get("retail_parity_accepted"),
            "source_residency": residency,
            "rendered_references": rendered_references,
            "unique_render_models": report.get("unique_render_models"),
            "rendered_mesh_instances": report.get("rendered_mesh_instances"),
            "shared_geometry_vertices": report.get("shared_geometry_vertices"),
            "shared_geometry_triangles": report.get("shared_geometry_triangles"),
            "unique_texture_samplers": report.get("unique_texture_samplers"),
        },
        "reference_membership": {
            "count": len(reference_keys),
            "sha256": digest_json(reference_keys),
        },
        "camera_transform": camera_transform,
        "scene_completion": completion_receipt,
        "poses": poses,
        "resources": resources,
        "unsupported_placement_count": sum(
            1 for row in placement_rows if row["status"] != "rendered-static-view"
        ),
        "canonical_reference_pose_status": "absent" if canonical_bindings is None else "observed",
        "canonical_player_pose_status": "absent",
    }


def scene_observations(evidence: dict, camera_request: dict, image: dict) -> dict:
    pose_status = "observed"
    if any(row["source_affine"] is None for row in evidence["poses"]["placements"]):
        pose_status = "partial"
    residency = evidence["resources"]["residency"]
    root_digest = digest_json(residency["root"])
    camera = evidence.get("camera_transform")
    camera_observation = {
        "status": "absent",
        "reason": ABSENT_OBSERVATIONS["actual_camera_transform"],
    }
    if isinstance(camera, dict):
        try:
            matrix = camera.get("world_from_camera")
            if not isinstance(matrix, list) or len(matrix) != 4:
                raise AcceptanceError("camera_transform.world_from_camera must be a 4x4 matrix")
            normalized_matrix = [
                _finite_vector(row, "camera_transform.world_from_camera row", 4)
                for row in matrix
            ]
            request_digest = digest_json({
                "source_position": _finite_vector(
                    camera_request.get("source_position"), "camera_request.source_position"
                ),
                "source_target": _finite_vector(
                    camera_request.get("source_target"), "camera_request.source_target"
                ),
            })
            if _integer(camera.get("scene_generation"), "camera_transform.scene_generation", 1) != residency["generation"]:
                raise AcceptanceError("camera transform belongs to a stale scene generation")
            if _sha256(camera.get("source_root_sha256"), "camera_transform.source_root_sha256") != root_digest:
                raise AcceptanceError("camera transform belongs to a different scene root")
            if _sha256(camera.get("camera_request_sha256"), "camera_transform.camera_request_sha256") != request_digest:
                raise AcceptanceError("camera transform does not match the requested camera")
            camera_observation = {
                "status": "observed",
                "scene_generation": residency["generation"],
                "source_root_sha256": root_digest,
                "camera_request_sha256": request_digest,
                "world_from_camera": normalized_matrix,
            }
        except AcceptanceError as error:
            camera_observation = {"status": "mismatch", "reason": str(error)}

    completion = evidence.get("scene_completion")
    completion_observation = {
        "status": "absent",
        "reason": "The host emitted no scene completion receipt.",
    }
    if isinstance(completion, dict):
        try:
            reference_count = evidence["reference_membership"]["count"]
            rendered_count = evidence["scene_report"]["rendered_references"]
            resource_count = sum(
                row["model"] is not None for row in evidence["resources"]["placements"]
            )
            if completion.get("state") != "complete":
                raise AcceptanceError("scene completion receipt is not complete")
            if _integer(completion.get("scene_generation"), "scene_completion.scene_generation", 1) != residency["generation"]:
                raise AcceptanceError("scene completion receipt belongs to a stale generation")
            if _sha256(completion.get("source_root_sha256"), "scene_completion.source_root_sha256") != root_digest:
                raise AcceptanceError("scene completion receipt belongs to a different scene root")
            for field, expected in (
                ("expected_references", reference_count),
                ("rendered_references", rendered_count),
                ("resource_references", resource_count),
            ):
                actual = _integer(completion.get(field), f"scene_completion.{field}", 0)
                if actual != expected:
                    raise AcceptanceError(f"scene completion receipt {field} does not match the scene report")
            if reference_count != rendered_count or rendered_count != resource_count:
                raise AcceptanceError("scene completion receipt records missing expected draws or resources")
            completion_observation = {
                "status": "observed",
                "scene_generation": residency["generation"],
                "source_root_sha256": root_digest,
                "expected_references": reference_count,
                "rendered_references": rendered_count,
                "resource_references": resource_count,
            }
        except AcceptanceError as error:
            completion_observation = {"status": "mismatch", "reason": str(error)}
    result = {
        "source_and_binary": {"status": "observed"},
        "input_profile": {"status": "observed"},
        "camera_request": {"status": "observed", "value": camera_request},
        "camera_transform": camera_observation,
        "scene_completion": completion_observation,
        "static_placement_pose": {
            "status": pose_status,
            "sha256": digest_json(evidence["poses"]),
            "count": len(evidence["poses"]["placements"]),
        },
        "canonical_player_pose": {
            "status": evidence["canonical_player_pose_status"],
            "reason": ABSENT_OBSERVATIONS["canonical_player_pose"]
        },
        "canonical_reference_pose": {
            "status": evidence["canonical_reference_pose_status"],
            "sha256": digest_json(evidence["poses"]["canonical_bindings"])
            if evidence["poses"]["canonical_bindings"] is not None else None,
        },
        "resource_manifest": {
            "status": "observed",
            "sha256": digest_json(evidence["resources"]),
            "model_count": len(evidence["resources"]["models"]),
            "placement_count": len(evidence["resources"]["placements"]),
        },
        "image": {"status": "observed", **image},
        "resource_drain": {"status": "absent", "reason": ABSENT_OBSERVATIONS["resource_drain"]},
        "frame_readiness_receipt": {
            "status": "absent",
            "reason": ABSENT_OBSERVATIONS["frame_readiness_receipt"],
        },
        "simulation_readiness": {
            "status": "unsupported" if evidence["scene_report"]["runtime_ready"] is False else "observed",
            "value": evidence["scene_report"]["runtime_ready"],
        },
    }
    return result


def _git_value(repo: Path, *arguments: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *arguments],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise AcceptanceError(f"Git command failed: {' '.join(arguments)}: {result.stderr.strip()}")
    return result.stdout.strip()


def _is_reparse_path(path: Path) -> bool:
    if path.is_symlink():
        return True
    is_junction = getattr(path, "is_junction", None)
    return bool(is_junction and is_junction())


def _source_relative_path(repo: Path, path: Path) -> str:
    try:
        relative = path.absolute().relative_to(repo.resolve()).as_posix()
    except ValueError as error:
        raise AcceptanceError(f"Production source escapes repository: {path}") from error
    if not relative or len(relative) > MAX_SOURCE_PATH_CHARS:
        raise AcceptanceError(f"Production source path is empty or too long: {path}")
    return relative


def _walk_source_root(repo: Path, relative_root: str, required: bool = True) -> list[Path]:
    raw_root = repo / relative_root
    if not raw_root.exists():
        if required:
            raise AcceptanceError(f"Production source root is missing: {relative_root}")
        return []
    if _is_reparse_path(raw_root):
        raise AcceptanceError(f"Production source root cannot be a symlink or junction: {relative_root}")
    root = _safe_child(repo, relative_root, "source")
    if not root.is_dir():
        raise AcceptanceError(f"Production source root is not a directory: {relative_root}")

    files: list[Path] = []

    def visit(directory: Path) -> None:
        try:
            with os.scandir(directory) as stream:
                entries = sorted(stream, key=lambda entry: (entry.name.casefold(), entry.name))
        except OSError as error:
            raise AcceptanceError(f"Cannot enumerate production source directory {directory}: {error}") from error
        for entry in entries:
            if entry.name.casefold() in SOURCE_IGNORED_DIRS and entry.is_dir(follow_symlinks=False):
                continue
            path = Path(entry.path)
            if _is_reparse_path(path):
                raise AcceptanceError(f"Production source cannot contain symlinks or junctions: {path}")
            if entry.is_dir(follow_symlinks=False):
                visit(path)
            elif entry.is_file(follow_symlinks=False):
                if path.suffix.casefold() != ".pyc":
                    files.append(path)
                    if len(files) > MAX_SOURCE_FILES:
                        raise AcceptanceError(
                            f"Production source exceeds the {MAX_SOURCE_FILES}-file inventory budget"
                        )
            else:
                raise AcceptanceError(f"Production source contains a non-regular input: {path}")

    visit(root)
    return files


def _read_source_toml(path: Path) -> dict:
    try:
        stat_before = path.stat()
        if stat_before.st_size > MAX_SOURCE_FILE_BYTES:
            raise AcceptanceError(f"Cargo manifest exceeds the source file budget: {path}")
        data = path.read_bytes()
        if len(data) > MAX_SOURCE_FILE_BYTES or len(data) != stat_before.st_size:
            raise AcceptanceError(f"Cargo manifest changed or exceeds the source file budget: {path}")
        return tomllib.loads(data.decode("utf-8"))
    except AcceptanceError:
        raise
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        raise AcceptanceError(f"Cannot read Cargo manifest {path}: {error}") from error


def _cargo_path_values(value: object):
    if isinstance(value, dict):
        path = value.get("path")
        if path is not None:
            yield path
        for child in value.values():
            yield from _cargo_path_values(child)
    elif isinstance(value, list):
        for child in value:
            yield from _cargo_path_values(child)


def _validate_source_references(repo: Path, files: list[Path], source_roots: list[Path]) -> None:
    file_paths = {_source_relative_path(repo, path) for path in files}
    for path in files:
        if path.suffix.casefold() != ".rs":
            continue
        try:
            if path.stat().st_size > MAX_SOURCE_FILE_BYTES:
                raise AcceptanceError(f"Rust source exceeds the source file budget: {path}")
            text = path.read_text(encoding="utf-8")
        except AcceptanceError:
            raise
        except (OSError, UnicodeError) as error:
            raise AcceptanceError(f"Cannot inspect Rust source references in {path}: {error}") from error

        path_calls = len(RUST_PATH_ATTRIBUTE_CALL.findall(text))
        path_matches = RUST_PATH_ATTRIBUTE.findall(text)
        include_calls = len(RUST_INCLUDE_CALL.findall(text))
        include_matches = RUST_INCLUDE_LITERAL.findall(text)
        if path_calls != len(path_matches) or include_calls != len(include_matches):
            raise AcceptanceError(f"Non-literal Rust source reference is outside the pinned closure: {path}")
        for reference in (*path_matches, *include_matches):
            requested = Path(reference)
            if requested.is_absolute():
                raise AcceptanceError(f"Rust source reference must be repository-relative: {path}: {reference}")
            raw_target = path.parent / requested
            try:
                if _is_reparse_path(raw_target):
                    raise AcceptanceError(f"Rust source reference cannot use a symlink or junction: {raw_target}")
                target = raw_target.resolve(strict=True)
            except AcceptanceError:
                raise
            except (OSError, RuntimeError) as error:
                raise AcceptanceError(f"Rust source reference is missing or invalid: {path}: {reference}") from error
            if not any(target == root or root in target.parents for root in source_roots):
                raise AcceptanceError(f"Rust source reference escapes the pinned source roots: {path}: {reference}")
            if not target.is_file():
                raise AcceptanceError(f"Rust source reference is not a file: {path}: {reference}")
            relative = _source_relative_path(repo, target)
            if relative not in file_paths:
                raise AcceptanceError(f"Rust source reference is absent from the inventory: {relative}")


def _source_file_record(path: Path, repo: Path, total_bytes: int) -> tuple[dict, int]:
    relative = _source_relative_path(repo, path)
    raw_path = repo / Path(relative)
    if _is_reparse_path(raw_path):
        raise AcceptanceError(f"Production source cannot be a symlink or junction: {relative}")
    safe_path = _safe_child(repo, relative, "source")
    try:
        before = safe_path.stat()
        if not stat.S_ISREG(before.st_mode):
            raise AcceptanceError(f"Production source is not a regular file: {relative}")
        if before.st_size > MAX_SOURCE_FILE_BYTES:
            raise AcceptanceError(f"Production source exceeds the per-file byte budget: {relative}")
        if total_bytes + before.st_size > MAX_SOURCE_BYTES:
            raise AcceptanceError(f"Production source exceeds the {MAX_SOURCE_BYTES}-byte inventory budget")
        digest = hashlib.sha256()
        read_bytes = 0
        with safe_path.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                read_bytes += len(block)
                if read_bytes > MAX_SOURCE_FILE_BYTES or total_bytes + read_bytes > MAX_SOURCE_BYTES:
                    raise AcceptanceError(f"Production source grew beyond its byte budget: {relative}")
                digest.update(block)
        after = safe_path.stat()
    except AcceptanceError:
        raise
    except OSError as error:
        raise AcceptanceError(f"Cannot hash production source {relative}: {error}") from error
    if (
        read_bytes != before.st_size
        or after.st_size != before.st_size
        or after.st_mtime_ns != before.st_mtime_ns
        or after.st_ino != before.st_ino
    ):
        raise AcceptanceError(f"Production source changed while being fingerprinted: {relative}")
    return {"path": relative, "bytes": read_bytes, "sha256": digest.hexdigest()}, total_bytes + read_bytes


def source_fingerprint(repo: Path) -> dict:
    repo = repo.resolve()
    source_roots: list[Path] = []
    source_paths: list[Path] = []
    for relative_root in SOURCE_ROOTS:
        root = _safe_child(repo, relative_root, "source")
        source_roots.append(root)
        source_paths.extend(_walk_source_root(repo, relative_root))
    for relative_root in SOURCE_OPTIONAL_ROOTS:
        source_paths.extend(_walk_source_root(repo, relative_root, required=False))
        optional_root = repo / relative_root
        if optional_root.is_dir():
            source_roots.append(_safe_child(repo, relative_root, "source"))
    for relative in SOURCE_CONTROL_FILES:
        raw_path = repo / relative
        if _is_reparse_path(raw_path):
            raise AcceptanceError(f"Source control input cannot be a symlink or junction: {relative}")
        path = _safe_child(repo, relative, "source")
        if not path.is_file():
            raise AcceptanceError(f"Source control input is missing: {relative}")
        source_paths.append(path)

    unique_paths: dict[str, Path] = {}
    for path in source_paths:
        relative = _source_relative_path(repo, path)
        folded = relative.casefold()
        if folded in unique_paths and unique_paths[folded] != path:
            raise AcceptanceError(f"Production source paths collide by case: {relative}")
        unique_paths[folded] = path
    ordered = sorted(
        unique_paths.values(),
        key=lambda path: (
            _source_relative_path(repo, path).casefold(),
            _source_relative_path(repo, path),
        ),
    )
    if len(ordered) > MAX_SOURCE_FILES:
        raise AcceptanceError(f"Production source exceeds the {MAX_SOURCE_FILES}-file inventory budget")

    file_names = {_source_relative_path(repo, path) for path in ordered}
    for manifest in (path for path in ordered if path.name == "Cargo.toml"):
        document = _read_source_toml(manifest)
        for value in _cargo_path_values(document):
            if not isinstance(value, str) or not value:
                raise AcceptanceError(f"Cargo path must be a non-empty string: {manifest}")
            requested = Path(value)
            if requested.is_absolute():
                raise AcceptanceError(f"Cargo source path must be repository-relative: {manifest}: {value}")
            raw_target = manifest.parent / requested
            try:
                if _is_reparse_path(raw_target):
                    raise AcceptanceError(f"Cargo source path cannot use a symlink or junction: {raw_target}")
                target = raw_target.resolve(strict=True)
            except AcceptanceError:
                raise
            except (OSError, RuntimeError) as error:
                raise AcceptanceError(f"Cargo source path is missing or invalid: {manifest}: {value}") from error
            if not any(target == root or root in target.parents for root in source_roots):
                raise AcceptanceError(f"Cargo source path escapes the pinned source roots: {manifest}: {value}")
            if target.is_file() and _source_relative_path(repo, target) not in file_names:
                raise AcceptanceError(f"Cargo source path is absent from the inventory: {target}")
            if target.is_dir():
                dependency_manifest = target / "Cargo.toml"
                if not dependency_manifest.is_file():
                    raise AcceptanceError(f"Cargo package path has no manifest: {target}")
                if _source_relative_path(repo, dependency_manifest) not in file_names:
                    raise AcceptanceError(f"Cargo package manifest is absent from the inventory: {target}")

    _validate_source_references(repo, ordered, source_roots)
    files = []
    total_bytes = 0
    for path in ordered:
        record, total_bytes = _source_file_record(path, repo, total_bytes)
        files.append(record)
    script_path = Path(__file__).resolve()
    return {
        "git_revision": _git_value(repo, "rev-parse", "HEAD"),
        "git_tree": _git_value(repo, "rev-parse", "HEAD^{tree}"),
        "source_roots": sorted(_source_relative_path(repo, path) for path in source_roots),
        "source_file_count": len(files),
        "source_total_bytes": total_bytes,
        "files": files,
        "harness_sha256": sha256_file(script_path),
    }


def _relative_repo_path(repo: Path, path: Path, name: str) -> str:
    resolved = path.resolve()
    root = repo.resolve()
    if root != resolved and root not in resolved.parents:
        raise AcceptanceError(f"{name} must be inside the assigned repository")
    return resolved.relative_to(root).as_posix()


def _check_private_output(repo: Path, install: Path, output: Path) -> Path:
    root = repo.resolve()
    private = (root / "local" / "v4-evidence").resolve()
    destination = output.resolve()
    if private != destination and private not in destination.parents:
        raise AcceptanceError("Capture output must stay under local/v4-evidence")
    install_root = install.resolve()
    if install_root == destination or install_root in destination.parents:
        raise AcceptanceError("Capture output must be outside the read-only installation")
    if destination.exists():
        raise AcceptanceError("Capture output directory already exists; stale output is refused")
    return destination


def _check_private_evidence_path(repo: Path, path: Path) -> Path:
    private = (repo.resolve() / "local" / "v4-evidence").resolve()
    destination = path.resolve()
    if private != destination and private not in destination.parents:
        raise AcceptanceError("Verification output must stay under local/v4-evidence")
    return destination


def _parse_png(data: bytes, max_pixels: int = 16_000_000) -> tuple[int, int, bytes]:
    signature = b"\x89PNG\r\n\x1a\n"
    if not data.startswith(signature):
        raise AcceptanceError("Capture is not a PNG")
    offset = len(signature)
    width = height = bit_depth = color_type = None
    compressed = bytearray()
    saw_end = False
    while offset + 12 <= len(data):
        length = int.from_bytes(data[offset : offset + 4], "big")
        if length > 64 * 1024 * 1024 or offset + 12 + length > len(data):
            raise AcceptanceError("PNG chunk length is invalid or exceeds limits")
        kind = data[offset + 4 : offset + 8]
        chunk = data[offset + 8 : offset + 8 + length]
        expected_crc = int.from_bytes(data[offset + 8 + length : offset + 12 + length], "big")
        actual_crc = zlib.crc32(kind + chunk) & 0xFFFFFFFF
        if actual_crc != expected_crc:
            raise AcceptanceError(f"PNG {kind!r} chunk has an invalid CRC")
        if kind == b"IHDR":
            if width is not None or length != 13:
                raise AcceptanceError("PNG must have one 13-byte IHDR")
            width = int.from_bytes(chunk[0:4], "big")
            height = int.from_bytes(chunk[4:8], "big")
            bit_depth, color_type = chunk[8], chunk[9]
            if chunk[10:] != b"\0\0\0":
                raise AcceptanceError("Unsupported PNG compression, filter, or interlace mode")
            if width <= 0 or height <= 0 or width * height > max_pixels:
                raise AcceptanceError("PNG dimensions are empty or exceed the pixel budget")
        elif kind == b"IDAT":
            if len(compressed) + length > 64 * 1024 * 1024:
                raise AcceptanceError("PNG compressed payload exceeds limits")
            compressed.extend(chunk)
        elif kind == b"IEND":
            if length != 0:
                raise AcceptanceError("PNG IEND chunk must be empty")
            saw_end = True
            offset += 12 + length
            break
        offset += 12 + length
    if not saw_end or width is None or height is None:
        raise AcceptanceError("PNG is missing its header or end marker")
    if bit_depth != 8 or color_type != 2:
        raise AcceptanceError("Only 8-bit RGB captures are supported")
    if offset != len(data):
        raise AcceptanceError("PNG has trailing bytes after IEND")
    stride = width * 3
    expected_size = height * (stride + 1)
    decompressor = zlib.decompressobj()
    try:
        raw = decompressor.decompress(bytes(compressed), expected_size + 1)
        if len(raw) <= expected_size:
            raw += decompressor.flush(expected_size + 1 - len(raw))
    except zlib.error as error:
        raise AcceptanceError(f"PNG image stream is invalid: {error}") from error
    if len(raw) != expected_size or not decompressor.eof or decompressor.unused_data or decompressor.unconsumed_tail:
        raise AcceptanceError("PNG decompressed size does not match its dimensions")
    pixels = bytearray(height * stride)
    source_offset = 0
    prior = bytearray(stride)
    for row_index in range(height):
        filter_type = raw[source_offset]
        source_offset += 1
        row = bytearray(raw[source_offset : source_offset + stride])
        source_offset += stride
        if filter_type == 1:
            for index in range(stride):
                left = row[index - 3] if index >= 3 else 0
                row[index] = (row[index] + left) & 0xFF
        elif filter_type == 2:
            for index in range(stride):
                row[index] = (row[index] + prior[index]) & 0xFF
        elif filter_type == 3:
            for index in range(stride):
                left = row[index - 3] if index >= 3 else 0
                row[index] = (row[index] + ((left + prior[index]) // 2)) & 0xFF
        elif filter_type == 4:
            for index in range(stride):
                left = row[index - 3] if index >= 3 else 0
                up = prior[index]
                upper_left = prior[index - 3] if index >= 3 else 0
                p = left + up - upper_left
                distances = (abs(p - left), abs(p - up), abs(p - upper_left))
                predictor = left if distances[0] <= distances[1] and distances[0] <= distances[2] else (
                    up if distances[1] <= distances[2] else upper_left
                )
                row[index] = (row[index] + predictor) & 0xFF
        elif filter_type != 0:
            raise AcceptanceError(f"Unsupported PNG row filter: {filter_type}")
        start = row_index * stride
        pixels[start : start + stride] = row
        prior = row
    return width, height, bytes(pixels)


def read_rgb_png(path: Path) -> tuple[int, int, bytes]:
    if path.stat().st_size > 64 * 1024 * 1024:
        raise AcceptanceError("PNG file exceeds the 64 MiB capture budget")
    return _parse_png(path.read_bytes())


def pixels_differ_from_first(pixels: bytes) -> int:
    if not pixels or len(pixels) % 3:
        return 0
    first = pixels[:3]
    return sum(pixels[index : index + 3] != first for index in range(0, len(pixels), 3))


def compare_png(expected_path: Path, actual_path: Path, tolerance: dict) -> dict:
    tolerance = _object(tolerance, "image tolerance")
    max_channel_delta = _integer(tolerance.get("max_channel_delta"), "max_channel_delta", 0, 255)
    max_changed_pixels = _integer(tolerance.get("max_changed_pixels"), "max_changed_pixels", 0)
    expected_width, expected_height, expected = read_rgb_png(expected_path)
    actual_width, actual_height, actual = read_rgb_png(actual_path)
    if (expected_width, expected_height) != (actual_width, actual_height):
        raise AcceptanceError("Capture dimensions changed")
    nonuniform_pixels = pixels_differ_from_first(actual)
    if nonuniform_pixels == 0:
        raise AcceptanceError("Capture is blank or has only one observed color")
    max_delta = 0
    changed_pixels = 0
    for offset in range(0, len(expected), 3):
        deltas = (
            abs(expected[offset] - actual[offset]),
            abs(expected[offset + 1] - actual[offset + 1]),
            abs(expected[offset + 2] - actual[offset + 2]),
        )
        max_delta = max(max_delta, *deltas)
        changed_pixels += int(any(delta != 0 for delta in deltas))
    if max_delta > max_channel_delta:
        raise AcceptanceError(
            f"Image maximum channel delta {max_delta} exceeds {max_channel_delta}"
        )
    if changed_pixels > max_changed_pixels:
        raise AcceptanceError(
            f"Image changed pixels {changed_pixels} exceeds {max_changed_pixels}"
        )
    return {
        "width": actual_width,
        "height": actual_height,
        "max_channel_delta": max_delta,
        "changed_pixels": changed_pixels,
        "pixel_count": actual_width * actual_height,
        "pixels_differing_from_first": nonuniform_pixels,
        "tolerance": tolerance,
    }


def validate_completion(manifest: object) -> dict[str, int | str]:
    doc = _object(manifest, "capture manifest")
    results = doc.get("scenarios")
    if not isinstance(results, list) or not results:
        raise AcceptanceError("Capture manifest has no per-run results")
    rows = [_object(row, f"scenarios[{index}]") for index, row in enumerate(results)]
    for index, row in enumerate(rows):
        if not isinstance(row.get("required"), bool):
            raise AcceptanceError(f"scenarios[{index}].required must be boolean")
    required = [row for row in rows if row["required"] is True]
    if not required:
        raise AcceptanceError("Capture manifest has no required scenarios")
    ids = [row.get("id") for row in rows]
    if any(not isinstance(item, str) or not item for item in ids) or len(set(ids)) != len(ids):
        raise AcceptanceError("Scenario result ids must be unique non-empty strings")
    states = [_string(row.get("state"), f"scenario {row.get('id')} state") for row in required]
    if all(state == "skipped" for state in states):
        raise AcceptanceError("An all-skipped run cannot pass")
    completed = sum(state == "completed" for state in states)
    failed = sum(state == "failed" for state in states)
    skipped = sum(state == "skipped" for state in states)
    if any(state not in ("completed", "failed", "skipped") for state in states):
        raise AcceptanceError("Required scenario has an unknown completion state")
    state = "complete" if completed == len(required) else "incomplete"
    declared = doc.get("completion")
    if declared is None:
        raise AcceptanceError("Capture manifest has no aggregate completion observation")
    declared = _object(declared, "completion")
    declared_required = _integer(declared.get("required"), "completion.required", 0)
    declared_completed = _integer(declared.get("completed"), "completion.completed", 0)
    declared_failed = _integer(declared.get("failed"), "completion.failed", 0)
    declared_skipped = _integer(declared.get("skipped"), "completion.skipped", 0)
    declared_state = _string(declared.get("state"), "completion.state")
    if (declared_required, declared_completed, declared_failed, declared_skipped, declared_state) != (
        len(required), completed, failed, skipped, state
    ):
        raise AcceptanceError("Aggregate completion disagrees with per-run results")
    return {"state": state, "required": len(required), "completed": completed, "failed": failed, "skipped": skipped}


def validate_manifest_files(manifest_path: Path, manifest: object) -> dict[str, Path]:
    doc = _object(manifest, "capture manifest")
    schema_version = _integer(doc.get("schema_version"), "manifest.schema_version")
    tool_version = _string(doc.get("tool_version"), "manifest.tool_version")
    if schema_version != SCHEMA_VERSION or tool_version != TOOL_VERSION:
        raise AcceptanceError("Capture manifest schema or harness version is unsupported")
    summary = validate_completion(doc)
    if summary["state"] != "complete":
        raise AcceptanceError("Every required per-run capture must complete before verification")
    root = manifest_path.resolve().parent
    paths = {}
    cell_editor_id = _string(doc.get("cell_editor_id"), "cell_editor_id")
    load_order_file = _object(doc.get("load_order_file"), "load_order_file")
    load_order_sha = _sha256(load_order_file.get("sha256"), "load_order_file.sha256")
    for row in doc["scenarios"]:
        scenario = _object(row, "scenario result")
        if scenario.get("state") != "completed":
            if scenario.get("required") is True:
                raise AcceptanceError(f"Required scenario did not complete: {scenario.get('id')}")
            continue
        interval = scenario.get("interval")
        interval = _object(interval, "scenario interval")
        start = interval.get("started_ns")
        finish = interval.get("finished_ns")
        start = _integer(start, f"{scenario.get('id')}.interval.started_ns", 1)
        finish = _integer(finish, f"{scenario.get('id')}.interval.finished_ns", start)
        files = _object(scenario.get("files"), "scenario files")
        image_path = validate_file_record(root, files.get("image"), start, finish, "image")
        report_path = validate_file_record(root, files.get("report"), start, finish, "report")
        validate_file_record(root, files.get("log"), start, finish, "log", allow_empty=True)
        width, height, _ = read_rgb_png(image_path)
        image_meta = _object(scenario.get("image"), "image metadata")
        image_width = _integer(image_meta.get("width"), f"{scenario.get('id')}.image.width", 1)
        image_height = _integer(image_meta.get("height"), f"{scenario.get('id')}.image.height", 1)
        if image_width != width or image_height != height:
            raise AcceptanceError(f"Image dimensions disagree with receipt for {scenario.get('id')}")
        if sha256_file(report_path) != scenario.get("report_sha256"):
            raise AcceptanceError(f"Scene report digest changed for {scenario.get('id')}")
        evidence = scene_evidence(read_json(report_path), cell_editor_id, load_order_sha)
        if not _strict_json_equal(evidence, scenario.get("evidence")):
            raise AcceptanceError(f"Scene evidence projection changed for {scenario.get('id')}")
        image_meta = _object(scenario.get("image"), "image metadata")
        camera_request = _object(scenario.get("camera_request"), "camera request")
        expected_observations = scene_observations(evidence, camera_request, image_meta)
        if not _strict_json_equal(expected_observations, scenario.get("observations")):
            raise AcceptanceError(f"Observation statuses changed for {scenario.get('id')}")
        paths[scenario["id"]] = image_path, report_path
    return paths


def compare_scene_evidence(expected: dict, actual: dict) -> None:
    for field in (
        "cell", "inputs", "camera_origin", "coordinates", "reference_membership",
        "camera_transform", "scene_completion", "scene_report", "poses", "resources",
    ):
        if not _strict_json_equal(expected.get(field), actual.get(field)):
            raise AcceptanceError(f"Frozen scene {field} changed")


def compare_capture_manifests(expected_path: Path, actual_path: Path) -> dict:
    expected = _object(read_json(expected_path), "expected capture manifest")
    actual = _object(read_json(actual_path), "actual capture manifest")
    expected_files = validate_manifest_files(expected_path, expected)
    actual_files = validate_manifest_files(actual_path, actual)
    completion = validate_completion(actual)
    failures = []
    if not _strict_json_equal(expected.get("source"), actual.get("source")):
        failures.append("source manifest changed")
    if not _strict_json_equal(expected.get("binary"), actual.get("binary")):
        failures.append("binary identity changed")
    if not _strict_json_equal(expected.get("config_sha256"), actual.get("config_sha256")):
        failures.append("scenario input changed")
    if not _strict_json_equal(expected.get("load_order_file"), actual.get("load_order_file")):
        failures.append("load-order input changed")
    expected_rows = {row["id"]: row for row in expected.get("scenarios", [])}
    actual_rows = {row["id"]: row for row in actual.get("scenarios", [])}
    if set(expected_rows) != set(actual_rows):
        failures.append("required scenario set changed")
    outcomes = []
    missing = []
    for scenario_id in sorted(set(expected_rows) & set(actual_rows)):
        old = expected_rows[scenario_id]
        new = actual_rows[scenario_id]
        checks = []
        try:
            if not _strict_json_equal(old.get("required"), new.get("required")):
                raise AcceptanceError("scenario required flag changed")
            if not _strict_json_equal(old.get("camera_request"), new.get("camera_request")):
                raise AcceptanceError("camera request changed")
            compare_scene_evidence(_object(old.get("evidence"), "expected evidence"), _object(new.get("evidence"), "actual evidence"))
            pixel_result = compare_png(
                expected_files[scenario_id][0],
                actual_files[scenario_id][0],
                _object(old.get("image_tolerance"), "image tolerance"),
            )
            checks.append({"name": "source/binary/input/camera/resource/pose pins", "status": "pass"})
            checks.append({"name": "image pixels", "status": "pass", **pixel_result})
            status = "pass"
        except (AcceptanceError, KeyError) as error:
            checks.append({"name": "frozen comparison", "status": "fail", "reason": str(error)})
            status = "fail"
            failures.append(f"{scenario_id}: {error}")
        observations = new.get("observations", {})
        if isinstance(observations, dict):
            for name, observation in observations.items():
                if isinstance(observation, dict) and observation.get("status") in (
                    "absent", "partial", "unsupported", "mismatch"
                ):
                    missing.append(f"{scenario_id}.{name}")
        for name in ("camera_transform", "scene_completion"):
            observation = observations.get(name) if isinstance(observations, dict) else None
            if not isinstance(observation, dict) or observation.get("status") != "observed":
                failures.append(f"{scenario_id}.{name} observation is absent, mismatched, or incomplete")
        outcomes.append({"id": scenario_id, "status": status, "checks": checks})
    required_expected = {row["id"] for row in expected.get("scenarios", []) if row.get("required") is True}
    required_actual = {row["id"] for row in actual.get("scenarios", []) if row.get("required") is True}
    if required_expected != required_actual:
        failures.append("required scenario set changed")
    harness_status = "fail" if failures else "pass"
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": "engineering_scene_replay_comparison",
        "harness_status": harness_status,
        "scene_status": "partial" if missing else "observed",
        "feature_acceptance": "not_accepted",
        "retail_parity_accepted": False,
        "completion": completion,
        "unobserved_or_partial": sorted(set(missing)),
        "failures": failures,
        "scenarios": outcomes,
        "expected_run_id": expected.get("run_id"),
        "actual_run_id": actual.get("run_id"),
    }


def _run_source_fingerprint(repo: Path, binary: Path) -> dict:
    source = source_fingerprint(repo)
    binary_relative = _relative_repo_path(repo, binary, "binary")
    return source, {
        "path": binary_relative,
        "bytes": binary.stat().st_size,
        "sha256": sha256_file(binary),
    }


def capture(args: argparse.Namespace) -> int:
    repo = args.repo.resolve()
    install = args.install.resolve()
    binary = args.binary.resolve()
    load_order = args.load_order.resolve()
    config_path = args.config.resolve()
    if not install.is_dir() or not binary.is_file() or not load_order.is_file() or not config_path.is_file():
        raise AcceptanceError("Install, binary, load-order, and scenario config inputs must exist")
    config = _object(read_json(config_path), "scenario config")
    runs = validate_capture_config(config)
    git_root = Path(_git_value(repo, "rev-parse", "--show-toplevel")).resolve()
    if git_root != repo:
        raise AcceptanceError("Repository path must be the assigned Git worktree root")
    if _git_value(repo, "branch", "--show-current") != "agents/v4-engine-acceptance":
        raise AcceptanceError("Capture must use the assigned engine-acceptance branch")
    _relative_repo_path(repo, binary, "binary")
    output = _check_private_output(repo, install, args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    run_id = str(uuid.uuid4())
    created_ns = time.time_ns()
    started_utc = utc_now()
    source, binary_pin = _run_source_fingerprint(repo, binary)
    load_order_hash = sha256_file(load_order)
    config_hash = sha256_file(config_path)
    scenario_results = []
    plugin_map = None

    for scenario in runs:
        name = scenario["id"]
        png = output / f"{name}.png"
        report_path = output / f"{name}.json"
        log_path = output / f"{name}.log"
        started_ns = time.time_ns()
        started_at = utc_now()
        command = [
            str(binary),
            "--install", str(install),
            "--cell", config["cell_editor_id"],
            "--load-order", str(load_order),
            "--camera-position", *[format(value, ".17g") for value in scenario["camera"]["source_position"]],
            "--camera-look-at", *[format(value, ".17g") for value in scenario["camera"]["source_target"]],
            "--headless",
            "--capture", str(png),
            "--report", str(report_path),
        ]
        exit_code = None
        error = None
        try:
            completed = subprocess.run(
                command,
                cwd=repo,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=args.timeout,
                check=False,
            )
            exit_code = completed.returncode
            log_path.write_bytes(completed.stdout)
            if exit_code != 0:
                error = f"preview exited with code {exit_code}"
            if exit_code == 0 and png.is_file() and report_path.is_file():
                if sha256_file(load_order) != load_order_hash:
                    raise AcceptanceError("Load-order input changed during the replay")
                report_value = read_json(report_path)
                evidence = scene_evidence(report_value, config["cell_editor_id"], load_order_hash)
                if plugin_map is None:
                    plugin_map = evidence["inputs"]["plugin_sha256"]
                elif plugin_map != evidence["inputs"]["plugin_sha256"]:
                    raise AcceptanceError("Installed plugin inputs changed between scenario runs")
                width, height, pixels = read_rgb_png(png)
                if (width, height) != (1280, 900):
                    raise AcceptanceError("Preview image dimensions differ from the pinned 1280x900 viewport")
                if pixels_differ_from_first(pixels) == 0:
                    raise AcceptanceError("Preview capture is blank or has only one observed color")
            elif exit_code == 0:
                error = "preview exited successfully without both a report and PNG"
        except subprocess.TimeoutExpired as exception:
            log_path.write_bytes(exception.stdout or b"")
            error = f"preview exceeded timeout of {args.timeout} seconds"
        except (OSError, AcceptanceError) as exception:
            error = str(exception)
        finished_ns = time.time_ns()
        finished_at = utc_now()
        row = {
            "id": name,
            "required": scenario["required"],
            "state": "completed" if error is None and exit_code == 0 else "failed",
            "exit_code": exit_code,
            "error": error,
            "started_at": started_at,
            "finished_at": finished_at,
            "interval": {"started_ns": started_ns, "finished_ns": finished_ns},
            "camera_request": scenario["camera"],
            "image_tolerance": scenario["image_tolerance"],
        }
        if row["state"] == "completed":
            try:
                report_record = _file_record(report_path, output, started_ns, finished_ns)
                image_record = _file_record(png, output, started_ns, finished_ns)
                log_record = _file_record(log_path, output, started_ns, finished_ns, allow_empty=True)
                report_value = read_json(report_path)
                evidence = scene_evidence(report_value, config["cell_editor_id"], load_order_hash)
                _, _, pixels = read_rgb_png(png)
                image_meta = {
                    "width": 1280,
                    "height": 900,
                    "sha256": image_record["sha256"],
                    "pixels_differing_from_first": pixels_differ_from_first(pixels),
                }
                camera_request = {
                    **scenario["camera"],
                    "source_origin": evidence["camera_origin"],
                    "coordinates": evidence["coordinates"],
                    "viewport_px": [1280, 900],
                    "projection": {
                        "kind": "perspective",
                        "near": 0.01,
                        "far": 10000.0,
                        "vertical_fov_status": "implicit Bevy 0.19.1 default; numeric value is not reported by the host",
                        "source": "crates/fallout-preview/src/main.rs::setup",
                    },
                }
                row.update(
                    camera_request=camera_request,
                    files={"image": image_record, "report": report_record, "log": log_record},
                    report_sha256=report_record["sha256"],
                    image=image_meta,
                    evidence=evidence,
                    observations=scene_observations(evidence, camera_request, image_meta),
                )
            except (OSError, AcceptanceError) as exception:
                row["state"] = "failed"
                row["error"] = str(exception)
        if row["state"] == "failed":
            attach_failed_artifacts(
                row,
                output,
                {"image": png, "report": report_path, "log": log_path},
                started_ns,
                finished_ns,
            )
        scenario_results.append(row)

    finished_ns = time.time_ns()
    if sha256_file(binary) != binary_pin["sha256"]:
        for row in scenario_results:
            row["state"] = "failed"
            row["error"] = "Preview executable changed during the replay"
    if sha256_file(load_order) != load_order_hash:
        for row in scenario_results:
            row["state"] = "failed"
            row["error"] = "Load-order input changed during the replay"
    final_source, final_binary = _run_source_fingerprint(repo, binary)
    if source != final_source or binary_pin != final_binary:
        for row in scenario_results:
            row["state"] = "failed"
            row["error"] = "Pinned source or executable changed during the replay"
    completed_count = sum(row["required"] and row["state"] == "completed" for row in scenario_results)
    failed_count = sum(row["required"] and row["state"] == "failed" for row in scenario_results)
    skipped_count = sum(row["required"] and row["state"] == "skipped" for row in scenario_results)
    completion = {
        "state": "complete" if completed_count == sum(row["required"] for row in scenario_results) else "incomplete",
        "required": sum(row["required"] for row in scenario_results),
        "completed": completed_count,
        "failed": failed_count,
        "skipped": skipped_count,
    }
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "tool_version": TOOL_VERSION,
        "kind": "frozen_scene_replay_candidate",
        "evidence_class": "engineering_static_scene",
        "run_id": run_id,
        "created_at": started_utc,
        "created_ns": created_ns,
        "finished_at": utc_now(),
        "finished_ns": finished_ns,
        "source": source,
        "binary": binary_pin,
        "cell_editor_id": config["cell_editor_id"],
        "viewport_px": [1280, 900],
        "config_sha256": config_hash,
        "load_order_file": {
            "path": _relative_repo_path(repo, load_order, "load-order")
            if repo == load_order or repo in load_order.parents
            else f"external/{load_order.name}",
            "sha256": load_order_hash,
        },
        "install_profile": "user-supplied New Vegas files; installation path intentionally omitted",
        "plugin_sha256": plugin_map,
        "completion": completion,
        "scenarios": scenario_results,
        "harness_status": "candidate" if completion["state"] == "complete" else "incomplete",
        "feature_acceptance": "not_accepted",
        "retail_parity_accepted": False,
        "scope_limits": [
            "Static interior preview only; not a gameplay route.",
            "Camera request is pinned, but the applied camera transform is not emitted.",
            "Post-exit resource drain and per-frame readiness are not emitted.",
            "Missing actor models, collision, behavior, original lighting, and simulation remain unsupported.",
        ],
    }
    write_json_new(output / "manifest.json", manifest)
    print(f"{completion['completed']}/{completion['required']} required scene captures completed.")
    print(f"Candidate manifest: {(output / 'manifest.json').relative_to(repo).as_posix()}")
    print(f"Harness candidate status: {manifest['harness_status']}; feature acceptance: not accepted.")
    return 0 if completion["state"] == "complete" else 1


def verify(args: argparse.Namespace) -> int:
    result = compare_capture_manifests(args.expected.resolve(), args.actual.resolve())
    repo = Path(__file__).resolve().parents[2]
    requested_result = args.result.resolve() if args.result else args.actual.resolve().parent / "verification.json"
    result_path = _check_private_evidence_path(repo, requested_result)
    write_json_new(result_path, result)
    print(f"Harness replay: {result['harness_status']}; scene observations: {result['scene_status']}.")
    print(f"Feature acceptance: {result['feature_acceptance']}; retail parity accepted: false.")
    if result["unobserved_or_partial"]:
        print("Absent or partial observations: " + ", ".join(result["unobserved_or_partial"]))
    if result["failures"]:
        for failure in result["failures"]:
            print("FAIL: " + failure)
    return 0 if result["harness_status"] == "pass" else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    capture_parser = commands.add_parser("capture", help="capture each configured preview scenario in a fresh private directory")
    capture_parser.add_argument("--repo", type=Path, required=True)
    capture_parser.add_argument("--binary", type=Path, required=True)
    capture_parser.add_argument("--install", type=Path, required=True)
    capture_parser.add_argument("--load-order", type=Path, required=True)
    capture_parser.add_argument("--config", type=Path, required=True)
    capture_parser.add_argument("--output-dir", type=Path, required=True)
    capture_parser.add_argument("--timeout", type=int, default=240)
    capture_parser.set_defaults(function=capture)
    verify_parser = commands.add_parser("verify", help="compare a fresh run against a frozen engineering capture")
    verify_parser.add_argument("--expected", type=Path, required=True)
    verify_parser.add_argument("--actual", type=Path, required=True)
    verify_parser.add_argument("--result", type=Path)
    verify_parser.set_defaults(function=verify)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        return args.function(args)
    except (AcceptanceError, OSError, subprocess.SubprocessError) as error:
        print(f"scene-replay: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
