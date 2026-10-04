#!/usr/bin/env python3
"""Collect and compare source-pinned Fallout behavior receipts.

The tool compares observations with a separately authored expectation. It does
not execute game logic, infer missing state, or award retail parity to fixtures.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any


CAPTURE_FORMAT = "rust-fallout.behavior-capture"
EXPECTATION_FORMAT = "rust-fallout.behavior-expectations"
SCHEMA_VERSION = 1
MAX_ARTIFACT_BYTES = 8 * 1024 * 1024
MAX_RECORDS = 100_000
MAX_ISSUES = 50
SHA256 = re.compile(r"^[0-9a-f]{64}$")
REVISION = re.compile(r"^[0-9a-f]{40}$")


class ReceiptError(ValueError):
    """Malformed, stale, incomplete, or unsupported receipt input."""


def _fail(message: str) -> None:
    raise ReceiptError(message)


def _pairs_without_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            _fail(f"duplicate JSON property: {key}")
        result[key] = value
    return result


def _reject_constant(value: str) -> None:
    _fail(f"non-finite JSON number is not allowed: {value}")


def _decode_json(data: bytes, label: str) -> Any:
    if len(data) > MAX_ARTIFACT_BYTES:
        _fail(f"{label} exceeds the {MAX_ARTIFACT_BYTES}-byte limit")
    try:
        return json.loads(
            data.decode("utf-8-sig"),
            object_pairs_hook=_pairs_without_duplicates,
            parse_constant=_reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        _fail(f"{label} is not valid UTF-8 JSON: {error}")


def load_json(path: Path) -> Any:
    try:
        return _decode_json(path.read_bytes(), str(path))
    except OSError as error:
        _fail(f"cannot read {path}: {error}")


def _canonical_bytes(value: Any) -> bytes:
    try:
        return json.dumps(
            value,
            ensure_ascii=False,
            allow_nan=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    except (TypeError, ValueError) as error:
        _fail(f"value cannot be canonically encoded: {error}")


def _sha256(value: Any) -> str:
    return hashlib.sha256(_canonical_bytes(value)).hexdigest()


def _is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _keys(value: Any, required: set[str], optional: set[str], path: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        _fail(f"{path} must be an object")
    missing = required - value.keys()
    extra = value.keys() - required - optional
    if missing:
        _fail(f"{path} is missing properties: {', '.join(sorted(missing))}")
    if extra:
        _fail(f"{path} has unknown properties: {', '.join(sorted(extra))}")
    return value


def _text(value: Any, path: str) -> str:
    if not isinstance(value, str) or not value.strip():
        _fail(f"{path} must be a non-empty string")
    return value


def _integer(value: Any, path: str, minimum: int = 0) -> int:
    if not _is_int(value) or value < minimum:
        _fail(f"{path} must be an integer >= {minimum}")
    return value


def _digest(value: Any, path: str) -> str:
    if not isinstance(value, str) or not SHA256.fullmatch(value):
        _fail(f"{path} must be a lowercase SHA-256 digest")
    return value


def _revision(value: Any, path: str) -> str:
    if not isinstance(value, str) or not REVISION.fullmatch(value):
        _fail(f"{path} must be a full lowercase Git revision")
    return value


def _utc(value: Any, path: str) -> datetime:
    if not isinstance(value, str):
        _fail(f"{path} must be an ISO-8601 timestamp with a timezone")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        _fail(f"{path} must be an ISO-8601 timestamp with a timezone")
    if parsed.tzinfo is None:
        _fail(f"{path} must include a timezone")
    return parsed


def _form_key(value: Any, path: str) -> None:
    key = _keys(value, {"profile", "origin_plugin", "local_id"}, set(), path)
    _text(key["profile"], f"{path}.profile")
    _text(key["origin_plugin"], f"{path}.origin_plugin")
    local_id = _integer(key["local_id"], f"{path}.local_id")
    if local_id > 0xFFFFFFFF:
        _fail(f"{path}.local_id exceeds a 32-bit source identity")


def _object_list(value: Any, path: str) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        _fail(f"{path} must be an array")
    if len(value) > MAX_RECORDS:
        _fail(f"{path} exceeds the {MAX_RECORDS}-record limit")
    for index, row in enumerate(value):
        if not isinstance(row, dict):
            _fail(f"{path}[{index}] must be an object")
    return value


def _validate_inventory(value: Any, path: str) -> None:
    rows = _object_list(value, path)
    seen: set[str] = set()
    for index, item in enumerate(rows):
        row_path = f"{path}[{index}]"
        item = _keys(
            item,
            {"lot_id", "owner", "base", "count", "facts"},
            set(),
            row_path,
        )
        lot_id = _text(item["lot_id"], f"{row_path}.lot_id")
        if lot_id in seen:
            _fail(f"{row_path}.lot_id duplicates an observed inventory lot")
        seen.add(lot_id)
        if not isinstance(item["owner"], dict):
            _fail(f"{row_path}.owner must retain its typed owner identity")
        _form_key(item["base"], f"{row_path}.base")
        _integer(item["count"], f"{row_path}.count", minimum=1)
        if not isinstance(item["facts"], dict):
            _fail(f"{row_path}.facts must preserve observed item facts")


def _validate_quest_state(value: Any, path: str) -> None:
    rows = _object_list(value, path)
    seen: set[tuple[str, str, int, str]] = set()
    for index, fact in enumerate(rows):
        row_path = f"{path}[{index}]"
        fact = _keys(fact, {"quest", "field", "value"}, set(), row_path)
        _form_key(fact["quest"], f"{row_path}.quest")
        field = _text(fact["field"], f"{row_path}.field")
        quest = fact["quest"]
        identity = (
            quest["profile"],
            quest["origin_plugin"],
            quest["local_id"],
            field,
        )
        if identity in seen:
            _fail(f"{row_path} duplicates a quest observation")
        seen.add(identity)


def _validate_timeline(timeline: Any) -> list[dict[str, Any]]:
    rows = _object_list(timeline, "timeline")
    if not rows:
        _fail("timeline must contain at least one observed record")
    ordinal = 0
    prior_tick = -1
    input_sequence = 0
    prior_state_revision = -1
    state_ids: set[str] = set()

    for index, row in enumerate(rows):
        path = f"timeline[{index}]"
        kind = _text(row.get("kind"), f"{path}.kind")
        if kind == "input":
            required = {
                "ordinal", "tick", "kind", "sequence", "action", "source",
                "context", "request_id", "outcome",
            }
        elif kind == "state":
            required = {
                "ordinal", "tick", "kind", "observation_id", "revision",
                "visible_revision", "snapshot_sha256", "inventory", "quest_state",
                "pending_events", "acknowledged_events",
            }
        elif kind == "event":
            required = {
                "ordinal", "tick", "kind", "event_sequence", "owner",
                "definition", "event_id", "status", "effects", "error",
            }
        elif kind == "acknowledgment":
            required = {
                "ordinal", "tick", "kind", "event_sequence", "instance_id",
                "definition", "before_revision", "after_revision", "status",
                "receipt_sha256",
            }
        elif kind == "save":
            required = {
                "ordinal", "tick", "kind", "operation", "request_id", "status",
                "generation", "state_revision", "receipt_sha256", "process_id",
            }
        else:
            _fail(f"{path}.kind is unsupported: {kind}")

        _keys(row, required, set(), path)
        ordinal += 1
        if _integer(row["ordinal"], f"{path}.ordinal", minimum=1) != ordinal:
            _fail(f"{path}.ordinal must be contiguous in observed order")
        tick = _integer(row["tick"], f"{path}.tick")
        if tick < prior_tick:
            _fail(f"{path}.tick moves backward")
        prior_tick = tick

        if kind == "input":
            input_sequence += 1
            if _integer(row["sequence"], f"{path}.sequence", minimum=1) != input_sequence:
                _fail(f"{path}.sequence must include every observed input in order")
            _text(row["action"], f"{path}.action")
            source = _keys(
                row["source"],
                {"kind", "device", "control", "device_id"},
                set(),
                f"{path}.source",
            )
            if source["kind"] not in {"physical", "host", "cli", "synthetic"}:
                _fail(f"{path}.source.kind is unsupported")
            for field in ("device", "control", "device_id"):
                if source[field] is not None and not isinstance(source[field], str):
                    _fail(f"{path}.source.{field} must be a string or null")
            context = _keys(
                row["context"],
                {"name", "focused", "scene_epoch", "expected_revision"},
                set(),
                f"{path}.context",
            )
            _text(context["name"], f"{path}.context.name")
            if not isinstance(context["focused"], bool):
                _fail(f"{path}.context.focused must be boolean")
            _integer(context["scene_epoch"], f"{path}.context.scene_epoch")
            _integer(context["expected_revision"], f"{path}.context.expected_revision")
            if row["request_id"] is not None:
                _text(row["request_id"], f"{path}.request_id")
            if row["outcome"] not in {"accepted", "refused", "ignored"}:
                _fail(f"{path}.outcome is unsupported")
        elif kind == "state":
            observation_id = _text(row["observation_id"], f"{path}.observation_id")
            if observation_id in state_ids:
                _fail(f"{path}.observation_id is duplicated")
            state_ids.add(observation_id)
            revision = _integer(row["revision"], f"{path}.revision")
            _integer(row["visible_revision"], f"{path}.visible_revision")
            if revision < prior_state_revision:
                _fail(f"{path}.revision moves backward")
            prior_state_revision = revision
            _digest(row["snapshot_sha256"], f"{path}.snapshot_sha256")
            _validate_inventory(row["inventory"], f"{path}.inventory")
            _validate_quest_state(row["quest_state"], f"{path}.quest_state")
            for field in ("pending_events", "acknowledged_events"):
                if not isinstance(row[field], list):
                    _fail(f"{path}.{field} must be an array")
            for pending_index, pending in enumerate(row["pending_events"]):
                pending_path = f"{path}.pending_events[{pending_index}]"
                pending = _keys(
                    pending,
                    {"sequence", "instance_id", "definition", "trigger"},
                    set(),
                    pending_path,
                )
                _integer(pending["sequence"], f"{pending_path}.sequence", minimum=1)
                _text(pending["instance_id"], f"{pending_path}.instance_id")
                _form_key(pending["definition"], f"{pending_path}.definition")
                if not isinstance(pending["trigger"], dict):
                    _fail(f"{pending_path}.trigger must be an object")
            for ack_index, sequence in enumerate(row["acknowledged_events"]):
                _integer(sequence, f"{path}.acknowledged_events[{ack_index}]", minimum=1)
        elif kind == "event":
            _integer(row["event_sequence"], f"{path}.event_sequence", minimum=1)
            if not isinstance(row["owner"], dict):
                _fail(f"{path}.owner must retain its typed owner identity")
            _form_key(row["definition"], f"{path}.definition")
            _text(row["event_id"], f"{path}.event_id")
            if row["status"] not in {"dispatched", "refused", "unsupported"}:
                _fail(f"{path}.status is unsupported")
            if not isinstance(row["effects"], list) or any(
                not isinstance(effect, dict) for effect in row["effects"]
            ):
                _fail(f"{path}.effects must be an array of observed effect objects")
            if row["error"] is not None and not isinstance(row["error"], str):
                _fail(f"{path}.error must be a string or null")
        elif kind == "acknowledgment":
            _integer(row["event_sequence"], f"{path}.event_sequence", minimum=1)
            _text(row["instance_id"], f"{path}.instance_id")
            _form_key(row["definition"], f"{path}.definition")
            _integer(row["before_revision"], f"{path}.before_revision")
            _integer(row["after_revision"], f"{path}.after_revision")
            if row["status"] not in {"acknowledged", "refused"}:
                _fail(f"{path}.status is unsupported")
            if row["receipt_sha256"] is not None:
                _digest(row["receipt_sha256"], f"{path}.receipt_sha256")
        else:
            if row["operation"] not in {"save", "continue"}:
                _fail(f"{path}.operation is unsupported")
            _text(row["request_id"], f"{path}.request_id")
            if row["status"] not in {"saved", "restored", "failed"}:
                _fail(f"{path}.status is unsupported")
            if row["generation"] is not None:
                _integer(row["generation"], f"{path}.generation", minimum=1)
            if row["state_revision"] is not None:
                _integer(row["state_revision"], f"{path}.state_revision")
            if row["receipt_sha256"] is not None:
                _digest(row["receipt_sha256"], f"{path}.receipt_sha256")
            _text(row["process_id"], f"{path}.process_id")
    return rows


def _validate_provenance(value: Any, evidence_class: str, path: str) -> dict[str, Any]:
    provenance = _keys(
        value,
        {
            "source_revision", "binary_sha256", "profile_id",
            "profile_fingerprint_sha256", "content_fingerprint_sha256", "process",
        },
        set(),
        path,
    )
    _revision(provenance["source_revision"], f"{path}.source_revision")
    _text(provenance["profile_id"], f"{path}.profile_id")
    for field in ("binary_sha256", "profile_fingerprint_sha256", "content_fingerprint_sha256"):
        if provenance[field] is not None:
            _digest(provenance[field], f"{path}.{field}")
    process = provenance["process"]
    if evidence_class == "synthetic_fixture":
        if process is not None:
            _fail(f"{path}.process must be null for a synthetic fixture")
    elif evidence_class == "engineering_capture":
        if any(
            provenance[field] is None
            for field in ("binary_sha256", "profile_fingerprint_sha256", "content_fingerprint_sha256")
        ):
            _fail(f"{path} requires executable, profile, and content fingerprints for an engineering capture")
        process = _keys(
            process,
            {"pid", "started_utc", "ended_utc", "exit_code"},
            set(),
            f"{path}.process",
        )
        _integer(process["pid"], f"{path}.process.pid", minimum=1)
        started = _utc(process["started_utc"], f"{path}.process.started_utc")
        ended = _utc(process["ended_utc"], f"{path}.process.ended_utc")
        if ended < started:
            _fail(f"{path}.process ended before it started")
        if not _is_int(process["exit_code"]) or process["exit_code"] != 0:
            _fail(f"{path}.process.exit_code must be zero for a complete capture")
    else:
        _fail(f"unsupported evidence_class in v1: {evidence_class}")
    return provenance


def validate_capture(value: Any) -> dict[str, Any]:
    capture = _keys(
        value,
        {
            "format", "schema_version", "scenario_id", "evidence_class", "complete",
            "skipped_steps", "dropped_records", "provenance", "trace_sha256", "timeline",
        },
        set(),
        "capture",
    )
    if (
        capture["format"] != CAPTURE_FORMAT
        or not _is_int(capture["schema_version"])
        or capture["schema_version"] != SCHEMA_VERSION
    ):
        _fail("capture format or schema_version is unsupported")
    _text(capture["scenario_id"], "capture.scenario_id")
    evidence_class = capture["evidence_class"]
    if evidence_class not in {"synthetic_fixture", "engineering_capture"}:
        _fail("capture evidence_class must be synthetic_fixture or engineering_capture")
    if capture["complete"] is not True:
        _fail("incomplete capture cannot pass")
    if _integer(capture["skipped_steps"], "capture.skipped_steps") != 0:
        _fail("capture skipped steps")
    if _integer(capture["dropped_records"], "capture.dropped_records") != 0:
        _fail("capture dropped records")
    _validate_provenance(capture["provenance"], evidence_class, "capture.provenance")
    timeline = _validate_timeline(capture["timeline"])
    actual_trace_digest = _sha256(timeline)
    if _digest(capture["trace_sha256"], "capture.trace_sha256") != actual_trace_digest:
        _fail("capture trace_sha256 does not match its timeline")
    return capture


def validate_expectation(value: Any) -> dict[str, Any]:
    expectation = _keys(
        value,
        {
            "format", "schema_version", "scenario_id", "evidence_class", "oracle",
            "provenance_pins", "minimum_counts", "expected_timeline",
        },
        set(),
        "expectation",
    )
    if (
        expectation["format"] != EXPECTATION_FORMAT
        or not _is_int(expectation["schema_version"])
        or expectation["schema_version"] != SCHEMA_VERSION
    ):
        _fail("expectation format or schema_version is unsupported")
    _text(expectation["scenario_id"], "expectation.scenario_id")
    evidence_class = expectation["evidence_class"]
    if evidence_class not in {"synthetic_fixture", "engineering_capture"}:
        _fail("expectation evidence_class must be synthetic_fixture or engineering_capture")
    oracle = _keys(expectation["oracle"], {"kind", "id", "sha256"}, set(), "expectation.oracle")
    expected_oracle = "independent_fixture" if evidence_class == "synthetic_fixture" else "frozen_engine_capture"
    if oracle["kind"] != expected_oracle:
        _fail(f"expectation.oracle.kind must be {expected_oracle} for this evidence class")
    _text(oracle["id"], "expectation.oracle.id")
    _digest(oracle["sha256"], "expectation.oracle.sha256")

    pins = _keys(
        expectation["provenance_pins"],
        {
            "source_revision", "binary_sha256", "profile_id",
            "profile_fingerprint_sha256", "content_fingerprint_sha256",
        },
        set(),
        "expectation.provenance_pins",
    )
    _revision(pins["source_revision"], "expectation.provenance_pins.source_revision")
    _text(pins["profile_id"], "expectation.provenance_pins.profile_id")
    for field in ("binary_sha256", "profile_fingerprint_sha256", "content_fingerprint_sha256"):
        if pins[field] is not None:
            _digest(pins[field], f"expectation.provenance_pins.{field}")
    if evidence_class == "engineering_capture" and (
        pins["binary_sha256"] is None
        or pins["profile_fingerprint_sha256"] is None
        or pins["content_fingerprint_sha256"] is None
    ):
        _fail("engineering expectations must pin the executable, profile, and content")

    minimums = _keys(
        expectation["minimum_counts"],
        {"input", "state", "event", "acknowledgment", "save"},
        set(),
        "expectation.minimum_counts",
    )
    if _integer(minimums["input"], "expectation.minimum_counts.input", minimum=1) < 1:
        _fail("expectations must require at least one input")
    if _integer(minimums["state"], "expectation.minimum_counts.state", minimum=1) < 1:
        _fail("expectations must require at least one state observation")
    for field in ("event", "acknowledgment", "save"):
        _integer(minimums[field], f"expectation.minimum_counts.{field}")

    timeline = _validate_timeline(expectation["expected_timeline"])
    if _sha256(timeline) != oracle["sha256"]:
        _fail("expectation oracle sha256 does not match expected_timeline")
    return expectation


def _count_kinds(timeline: list[dict[str, Any]]) -> dict[str, int]:
    return {
        "input": sum(row["kind"] == "input" for row in timeline),
        "state": sum(row["kind"] == "state" for row in timeline),
        "event": sum(row["kind"] == "event" for row in timeline),
        "acknowledgment": sum(row["kind"] == "acknowledgment" for row in timeline),
        "save": sum(row["kind"] == "save" for row in timeline),
    }


def _short(value: Any) -> str:
    rendered = json.dumps(value, ensure_ascii=False, sort_keys=True, allow_nan=False)
    return rendered if len(rendered) <= 512 else rendered[:509] + "..."


def _diff(expected: Any, actual: Any, path: str, out: list[dict[str, Any]]) -> None:
    if len(out) >= MAX_ISSUES:
        return
    if type(expected) is not type(actual):
        out.append({"path": path, "expected": _short(expected), "actual": _short(actual)})
        return
    if isinstance(expected, dict) and isinstance(actual, dict):
        for key in sorted(expected.keys() | actual.keys()):
            child = f"{path}/{str(key).replace('~', '~0').replace('/', '~1')}"
            if key not in expected:
                out.append({"path": child, "expected": "<absent>", "actual": _short(actual[key])})
            elif key not in actual:
                out.append({"path": child, "expected": _short(expected[key]), "actual": "<absent>"})
            else:
                _diff(expected[key], actual[key], child, out)
        return
    if isinstance(expected, list) and isinstance(actual, list):
        if len(expected) != len(actual):
            out.append({"path": path, "expected_length": len(expected), "actual_length": len(actual)})
        for index, (want, got) in enumerate(zip(expected, actual)):
            _diff(want, got, f"{path}/{index}", out)
        return
    if expected != actual:
        out.append({"path": path, "expected": _short(expected), "actual": _short(actual)})


def compare(capture_value: Any, expectation_value: Any) -> dict[str, Any]:
    try:
        capture = validate_capture(capture_value)
        expectation = validate_expectation(expectation_value)
    except ReceiptError as error:
        return {
            "schema_version": SCHEMA_VERSION,
            "status": "failed",
            "classification": "invalid_receipt",
            "retail_pass": False,
            "mismatches": [{"path": "/", "reason": str(error)}],
        }

    mismatches: list[dict[str, Any]] = []
    for field in ("scenario_id", "evidence_class"):
        if capture[field] != expectation[field]:
            mismatches.append({
                "path": f"/{field}",
                "expected": _short(expectation[field]),
                "actual": _short(capture[field]),
            })
    for field, expected in expectation["provenance_pins"].items():
        actual = capture["provenance"].get(field)
        if actual != expected:
            mismatches.append({
                "path": f"/provenance/{field}",
                "expected": _short(expected),
                "actual": _short(actual),
            })

    counts = _count_kinds(capture["timeline"])
    for kind, minimum in expectation["minimum_counts"].items():
        if counts[kind] < minimum:
            mismatches.append({
                "path": f"/timeline/{kind}",
                "minimum": minimum,
                "actual": counts[kind],
            })

    _diff(expectation["expected_timeline"], capture["timeline"], "/timeline", mismatches)
    passed = not mismatches
    evidence_class = capture["evidence_class"]
    classification = (
        "synthetic_fixture_validation" if evidence_class == "synthetic_fixture"
        else "engineering_replay_validation"
    )
    return {
        "schema_version": SCHEMA_VERSION,
        "scenario_id": capture["scenario_id"],
        "status": "passed" if passed else "failed",
        "classification": classification,
        "retail_pass": False,
        "counts": counts,
        "mismatches": mismatches[:MAX_ISSUES],
    }


def _manifest(value: Any) -> dict[str, Any]:
    manifest = _keys(
        value,
        {
            "scenario_id", "evidence_class", "complete", "skipped_steps",
            "dropped_records", "provenance",
        },
        set(),
        "manifest",
    )
    _text(manifest["scenario_id"], "manifest.scenario_id")
    if manifest["evidence_class"] not in {"synthetic_fixture", "engineering_capture"}:
        _fail("manifest evidence_class must be synthetic_fixture or engineering_capture")
    if not isinstance(manifest["complete"], bool):
        _fail("manifest.complete must be boolean")
    _integer(manifest["skipped_steps"], "manifest.skipped_steps")
    _integer(manifest["dropped_records"], "manifest.dropped_records")
    _validate_provenance(manifest["provenance"], manifest["evidence_class"], "manifest.provenance")
    return manifest


def _read_jsonl(path: Path) -> list[dict[str, Any]]:
    try:
        data = path.read_bytes()
    except OSError as error:
        _fail(f"cannot read {path}: {error}")
    if len(data) > MAX_ARTIFACT_BYTES:
        _fail(f"{path} exceeds the {MAX_ARTIFACT_BYTES}-byte limit")
    try:
        lines = data.decode("utf-8-sig").splitlines()
    except UnicodeDecodeError as error:
        _fail(f"{path} is not valid UTF-8: {error}")
    if not lines or any(not line.strip() for line in lines):
        _fail(f"{path} must contain non-empty JSONL records with no blank lines")
    if len(lines) > MAX_RECORDS:
        _fail(f"{path} exceeds the {MAX_RECORDS}-record limit")
    rows = []
    for index, line in enumerate(lines):
        row = _decode_json(line.encode("utf-8"), f"{path} line {index + 1}")
        if not isinstance(row, dict):
            _fail(f"{path} line {index + 1} must be an object")
        rows.append(row)
    return rows


def _write_new(path: Path, value: Any) -> None:
    payload = (json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")
    if len(payload) > MAX_ARTIFACT_BYTES:
        _fail(f"output exceeds the {MAX_ARTIFACT_BYTES}-byte limit")
    try:
        with path.open("xb") as output:
            output.write(payload)
            output.flush()
    except FileExistsError:
        _fail(f"refusing to overwrite existing evidence: {path}")
    except OSError as error:
        _fail(f"cannot write {path}: {error}")


def collect(manifest_path: Path, timeline_path: Path, output_path: Path) -> dict[str, Any]:
    manifest = _manifest(load_json(manifest_path))
    timeline = _validate_timeline(_read_jsonl(timeline_path))
    capture = {
        "format": CAPTURE_FORMAT,
        "schema_version": SCHEMA_VERSION,
        **manifest,
        "trace_sha256": _sha256(timeline),
        "timeline": timeline,
    }
    validate_capture(capture)
    _write_new(output_path, capture)
    return {"status": "captured", "scenario_id": capture["scenario_id"], "trace_sha256": capture["trace_sha256"], "output": str(output_path)}


def _cli() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    collect_parser = commands.add_parser("capture", help="pack a run manifest and ordered host JSONL into a receipt")
    collect_parser.add_argument("--manifest", type=Path, required=True)
    collect_parser.add_argument("--timeline", type=Path, required=True)
    collect_parser.add_argument("--output", type=Path, required=True)
    compare_parser = commands.add_parser("replay", help="compare one capture with an independent expectation")
    compare_parser.add_argument("capture", type=Path)
    compare_parser.add_argument("expectation", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "capture":
            result = collect(args.manifest, args.timeline, args.output)
            code = 0
        else:
            result = compare(load_json(args.capture), load_json(args.expectation))
            code = 0 if result["status"] == "passed" else 1
    except ReceiptError as error:
        result = {
            "schema_version": SCHEMA_VERSION,
            "status": "failed",
            "classification": "invalid_receipt",
            "retail_pass": False,
            "mismatches": [{"path": "/", "reason": str(error)}],
        }
        code = 2
    sys.stdout.write(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False) + "\n")
    return code


if __name__ == "__main__":
    raise SystemExit(_cli())
