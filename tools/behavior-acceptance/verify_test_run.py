#!/usr/bin/env python3
"""Verify a frozen Rust test run against pinned package and case evidence.

This checks captured test-run integrity and consistency. It does not execute the
binary or attest that the captured output came from it.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

from replay import MAX_ARTIFACT_BYTES, ReceiptError, load_json


FORMAT = "rust-fallout.frozen-test-expectation"
SCHEMA_VERSION = 1
SHA256 = re.compile(r"^[0-9a-f]{64}$")
REVISION = re.compile(r"^[0-9a-f]{40}$")
RUNNING = re.compile(r"^running ([0-9]+) tests?$")
CASE = re.compile(r"^test (.+?) \.\.\. (ok|ignored(?:, .*)?|FAILED)$")
SUMMARY = re.compile(
    r"^test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; "
    r"([0-9]+) ignored;"
)
FOCUSED_ADMISSION = re.compile(r"(?im)^.*focused slot acquired \([0-9]+/[0-9]+\)\s*$")


def _fail(message: str) -> None:
    raise ReceiptError(message)


def _object(
    value: Any, label: str, required: set[str], optional: set[str] | None = None
) -> dict[str, Any]:
    if not isinstance(value, dict):
        _fail(f"{label} must be an object")
    allowed_optional = optional or set()
    missing = required - value.keys()
    extra = value.keys() - required - allowed_optional
    if missing:
        _fail(f"{label} is missing properties: {', '.join(sorted(missing))}")
    if extra:
        _fail(f"{label} has unknown properties: {', '.join(sorted(extra))}")
    return value


def _text(value: Any, path: str) -> str:
    if not isinstance(value, str) or not value.strip():
        _fail(f"{path} must be a non-empty string")
    return value


def _integer(value: Any, path: str, minimum: int = 0) -> int:
    if type(value) is not int or value < minimum:
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


def _hash_file(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    try:
        with path.open("rb") as source:
            while chunk := source.read(1024 * 1024):
                size += len(chunk)
                digest.update(chunk)
    except OSError as error:
        _fail(f"cannot read {path}: {error}")
    return digest.hexdigest(), size


def _read_log(path: Path) -> bytes:
    try:
        with path.open("rb") as source:
            data = source.read(MAX_ARTIFACT_BYTES + 1)
    except OSError as error:
        _fail(f"cannot read log {path}: {error}")
    if len(data) > MAX_ARTIFACT_BYTES:
        _fail(f"log {path} exceeds the {MAX_ARTIFACT_BYTES}-byte limit")
    return data


def _within(root: Path, raw_path: Any, label: str) -> Path:
    value = _text(raw_path, label)
    candidate = Path(value)
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        resolved = candidate.resolve(strict=True)
    except OSError as error:
        _fail(f"cannot resolve {label}: {error}")
    if not resolved.is_relative_to(root):
        _fail(f"{label} escapes its permitted root")
    return resolved


def _same_path(left: Path, right: Path) -> bool:
    return os.path.normcase(str(left)) == os.path.normcase(str(right))


def _parse_harness_output(text: str) -> tuple[dict[str, str], dict[str, int], str]:
    running_counts: list[int] = []
    summaries: list[tuple[str, int, int, int]] = []
    cases: dict[str, str] = {}

    for line in text.splitlines():
        running = RUNNING.fullmatch(line.strip())
        if running:
            running_counts.append(int(running.group(1)))
            continue

        summary = SUMMARY.match(line.strip())
        if summary:
            summaries.append(
                (
                    summary.group(1),
                    int(summary.group(2)),
                    int(summary.group(3)),
                    int(summary.group(4)),
                )
            )
            continue

        if line.startswith("test "):
            case = CASE.fullmatch(line.strip())
            if case is None:
                _fail(f"unrecognized Rust test result line: {line[:200]}")
            name, status_text = case.groups()
            if name in cases:
                _fail(f"duplicate Rust test result: {name}")
            if status_text == "FAILED":
                cases[name] = "failed"
            elif status_text.startswith("ignored"):
                cases[name] = "ignored"
            else:
                cases[name] = "ok"

    if len(running_counts) != 1:
        _fail("stdout must contain exactly one Rust test harness start")
    if len(summaries) != 1:
        _fail("stdout must contain exactly one Rust test result summary")
    if not cases:
        _fail("stdout contains no Rust test case results")

    announced_count = running_counts[0]
    summary_status, passed, failed, ignored = summaries[0]
    counts = {
        "passed": sum(status == "ok" for status in cases.values()),
        "failed": sum(status == "failed" for status in cases.values()),
        "ignored": sum(status == "ignored" for status in cases.values()),
    }
    if announced_count != len(cases):
        _fail("Rust harness announced a different test count than its case lines")
    if counts != {"passed": passed, "failed": failed, "ignored": ignored}:
        _fail("Rust test case lines disagree with the summary counts")
    return cases, counts, summary_status


def _validate_expectation(value: Any) -> dict[str, Any]:
    expected = _object(
        value,
        "expectation",
        {
            "format", "schema_version", "scenario_id", "lane", "producer_head",
            "producer_package_sha256", "binary_relative_path", "binary_sha256",
            "test_cases", "require_empty_stderr",
        },
    )
    if (
        expected["format"] != FORMAT
        or type(expected["schema_version"]) is not int
        or expected["schema_version"] != SCHEMA_VERSION
    ):
        _fail("expectation format or schema_version is unsupported")
    _text(expected["scenario_id"], "expectation.scenario_id")
    _text(expected["lane"], "expectation.lane")
    _revision(expected["producer_head"], "expectation.producer_head")
    _digest(expected["producer_package_sha256"], "expectation.producer_package_sha256")
    relative = Path(_text(expected["binary_relative_path"], "expectation.binary_relative_path"))
    if relative.is_absolute() or ".." in relative.parts:
        _fail("expectation.binary_relative_path must stay within the producer root")
    _digest(expected["binary_sha256"], "expectation.binary_sha256")
    if not isinstance(expected["require_empty_stderr"], bool):
        _fail("expectation.require_empty_stderr must be boolean")

    rows = expected["test_cases"]
    if not isinstance(rows, list) or not rows:
        _fail("expectation.test_cases must be a non-empty array")
    seen: set[str] = set()
    for index, row in enumerate(rows):
        row = _object(row, f"expectation.test_cases[{index}]", {"name", "status"})
        name = _text(row["name"], f"expectation.test_cases[{index}].name")
        if name in seen:
            _fail(f"expectation duplicates test case: {name}")
        seen.add(name)
        status = _text(row["status"], f"expectation.test_cases[{index}].status")
        if status not in {"ok", "ignored"}:
            _fail(f"expectation.test_cases[{index}].status must be ok or ignored")
    return expected


def _validate_artifact_bundle(
    package_value: Any,
    receipt_value: Any,
    expectation: dict[str, Any],
    package_sha256: str,
    producer_root: Path,
    evidence_root: Path,
) -> tuple[dict[str, Any], bytes]:
    package = _object(
        package_value,
        "package",
        {"head", "source_pins", "artifacts"},
        {
            "schema_version", "generation", "run_id", "lane", "session_uuid",
            "task_id", "created_at", "base", "changed_paths", "problem",
            "behavior", "consumer", "checks", "shared_wiring", "limitations",
            "next_action",
        },
    )
    receipt = _object(
        receipt_value,
        "receipt",
        {
            "schema_version", "producer_head", "producer_package_sha256",
            "source_pins_verified", "binary", "binary_sha256_before",
            "binary_sha256_after", "argv", "wrapper_exit_code", "cargo_invoked",
            "stdout_path", "stdout_sha256", "stdout_bytes", "stderr_path",
            "stderr_sha256", "stderr_bytes",
        },
        {"run_id", "task_id", "attempt_directory"},
    )
    if type(receipt["schema_version"]) is not int or receipt["schema_version"] != 1:
        _fail("receipt schema_version is unsupported")
    _integer(receipt["wrapper_exit_code"], "receipt.wrapper_exit_code")
    head = _revision(package["head"], "package.head")
    if head != expectation["producer_head"] or receipt["producer_head"] != head:
        _fail("producer head does not match the independent expectation")
    if package_sha256 != expectation["producer_package_sha256"]:
        _fail("producer package SHA-256 does not match the independent expectation")
    if receipt["producer_package_sha256"] != package_sha256:
        _fail("receipt package SHA-256 does not match the package file")
    if receipt["source_pins_verified"] is not True:
        _fail("receipt does not attest source pin preflight")
    if receipt["cargo_invoked"] is not False:
        _fail("frozen test receipt must prove Cargo was not invoked")

    pins = package["source_pins"]
    if not isinstance(pins, list) or not pins:
        _fail("package.source_pins must be a non-empty array")
    seen_pins: set[str] = set()
    for index, raw_pin in enumerate(pins):
        pin = _object(raw_pin, f"package.source_pins[{index}]", {"path", "sha256"}, {"bytes"})
        relative = Path(_text(pin["path"], f"package.source_pins[{index}].path"))
        if relative.is_absolute() or ".." in relative.parts:
            _fail(f"package.source_pins[{index}].path escapes producer root")
        pin_path = _within(producer_root, pin["path"], f"package.source_pins[{index}].path")
        identity = os.path.normcase(str(pin_path))
        if identity in seen_pins:
            _fail(f"package.source_pins duplicates {pin['path']}")
        seen_pins.add(identity)
        wanted = _digest(pin["sha256"], f"package.source_pins[{index}].sha256")
        actual, size = _hash_file(pin_path)
        if actual != wanted:
            _fail(f"source pin mismatch: {pin['path']}")
        if "bytes" in pin and _integer(pin["bytes"], f"package.source_pins[{index}].bytes") != size:
            _fail(f"source pin byte count mismatch: {pin['path']}")

    binary_expected = _within(
        producer_root, expectation["binary_relative_path"], "expectation.binary_relative_path"
    )
    binary_actual = _within(producer_root, receipt["binary"], "receipt.binary")
    if not _same_path(binary_expected, binary_actual):
        _fail("receipt binary path differs from the independent expectation")
    actual_binary_hash, binary_size = _hash_file(binary_actual)
    wanted_binary_hash = _digest(expectation["binary_sha256"], "expectation.binary_sha256")
    if actual_binary_hash != wanted_binary_hash:
        _fail("frozen binary hash differs from the independent expectation")
    for field in ("binary_sha256_before", "binary_sha256_after"):
        if _digest(receipt[field], f"receipt.{field}") != wanted_binary_hash:
            _fail(f"receipt.{field} differs from the frozen binary pin")

    artifacts = package["artifacts"]
    if not isinstance(artifacts, list):
        _fail("package.artifacts must be an array")
    matches = []
    for index, raw_artifact in enumerate(artifacts):
        artifact = _object(
            raw_artifact, f"package.artifacts[{index}]", {"path", "sha256"}, {"bytes"}
        )
        artifact_path = _within(
            producer_root, artifact["path"], f"package.artifacts[{index}].path"
        )
        if _same_path(artifact_path, binary_actual):
            matches.append(artifact)
    if len(matches) != 1:
        _fail("producer package must contain exactly one matching frozen binary artifact")
    artifact = matches[0]
    if _digest(artifact["sha256"], "package binary artifact sha256") != wanted_binary_hash:
        _fail("producer package binary artifact hash does not match the expectation")
    if "bytes" in artifact and _integer(artifact["bytes"], "package binary artifact bytes") != binary_size:
        _fail("producer package binary artifact byte count mismatch")

    argv = receipt["argv"]
    if not isinstance(argv, list) or not argv or any(not isinstance(arg, str) for arg in argv):
        _fail("receipt.argv must be a non-empty string array")
    for flag in ("--lane", "--session", "--wait", "--"):
        if flag not in argv:
            _fail("receipt.argv does not show central focused admission arguments")
    lane_index = argv.index("--lane")
    session_index = argv.index("--session")
    separator = argv.index("--")
    if lane_index + 1 >= len(argv) or argv[lane_index + 1] != expectation["lane"]:
        _fail("receipt.argv lane differs from the independent expectation")
    if session_index + 1 >= len(argv) or not argv[session_index + 1].strip():
        _fail("receipt.argv is missing the assigned session UUID")
    if not any(Path(arg).name.lower() == "team-build.py" for arg in argv[:separator]):
        _fail("receipt.argv does not invoke the central team-build.py wrapper")
    if separator + 2 >= len(argv) or not _same_path(Path(argv[separator + 1]), binary_actual):
        _fail("receipt.argv does not execute the pinned frozen binary")
    if "--nocapture" not in argv[separator + 2:]:
        _fail("receipt.argv must preserve Rust test output with --nocapture")
    if any("cargo" in arg.lower() for arg in argv[separator + 1:]):
        _fail("receipt.argv invokes Cargo instead of the frozen binary")

    stdout_path = _within(evidence_root, receipt["stdout_path"], "receipt.stdout_path")
    stderr_path = _within(evidence_root, receipt["stderr_path"], "receipt.stderr_path")
    stdout = _read_log(stdout_path)
    stderr = _read_log(stderr_path)
    for label, data, digest_field, bytes_field in (
        ("stdout", stdout, "stdout_sha256", "stdout_bytes"),
        ("stderr", stderr, "stderr_sha256", "stderr_bytes"),
    ):
        expected_digest = _digest(receipt[digest_field], f"receipt.{digest_field}")
        if hashlib.sha256(data).hexdigest() != expected_digest:
            _fail(f"receipt {label} hash does not match its file")
        if _integer(receipt[bytes_field], f"receipt.{bytes_field}") != len(data):
            _fail(f"receipt {label} byte count does not match its file")
    if not FOCUSED_ADMISSION.search(stdout.decode("utf-8-sig", errors="replace")):
        _fail("stdout does not show focused-slot admission")
    if expectation["require_empty_stderr"] and stderr:
        _fail("stderr must be empty for this expectation")
    return receipt, stdout


def verify(
    package_path: Path,
    receipt_path: Path,
    expectation_path: Path,
    producer_root: Path,
    evidence_root: Path,
) -> dict[str, Any]:
    try:
        producer_root = producer_root.resolve(strict=True)
        evidence_root = evidence_root.resolve(strict=True)
        package_path = _within(producer_root, package_path, "package path")
        receipt_path = _within(evidence_root, receipt_path, "receipt path")
        expectation_path = _within(evidence_root, expectation_path, "expectation path")
        package_data = _read_log(package_path)
        package_sha256 = hashlib.sha256(package_data).hexdigest()
        expectation = _validate_expectation(load_json(expectation_path))
        package = load_json(package_path)
        receipt_value = load_json(receipt_path)
        receipt, stdout = _validate_artifact_bundle(
            package, receipt_value, expectation, package_sha256, producer_root, evidence_root
        )
        cases, counts, summary_status = _parse_harness_output(stdout.decode("utf-8-sig"))
    except (ReceiptError, UnicodeDecodeError, OSError, RuntimeError, ValueError) as error:
        return {
            "schema_version": SCHEMA_VERSION,
            "status": "failed",
            "classification": "invalid_receipt",
            "retail_pass": False,
            "mismatches": [{"path": "/", "reason": str(error)}],
        }

    mismatches: list[dict[str, Any]] = []
    exit_code = _integer(receipt["wrapper_exit_code"], "receipt.wrapper_exit_code")
    if exit_code != 0:
        mismatches.append({"path": "/wrapper_exit_code", "expected": 0, "actual": exit_code})
    if summary_status != "ok" or counts["failed"] != 0:
        mismatches.append({
            "path": "/test_results",
            "expected": "all tests passed",
            "actual": {"summary": summary_status, **counts},
        })
    if receipt["stderr_bytes"] != 0:
        mismatches.append({"path": "/stderr_bytes", "expected": 0, "actual": receipt["stderr_bytes"]})

    expected_cases = {row["name"]: row["status"] for row in expectation["test_cases"]}
    if cases.keys() != expected_cases.keys():
        mismatches.append({
            "path": "/test_cases",
            "missing": sorted(expected_cases.keys() - cases.keys()),
            "unexpected": sorted(cases.keys() - expected_cases.keys()),
        })
    for name in sorted(cases.keys() & expected_cases.keys()):
        if cases[name] != expected_cases[name]:
            mismatches.append({
                "path": f"/test_cases/{name}",
                "expected": expected_cases[name],
                "actual": cases[name],
            })

    return {
        "schema_version": SCHEMA_VERSION,
        "scenario_id": expectation["scenario_id"],
        "status": "passed" if not mismatches else "failed",
        "classification": "frozen_test_artifact_validation",
        "retail_pass": False,
        "producer_head": expectation["producer_head"],
        "binary_sha256": expectation["binary_sha256"],
        "test_counts": counts,
        "mismatches": mismatches[:50],
    }


def _cli() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--expectation", type=Path, required=True)
    parser.add_argument("--producer-root", type=Path, required=True)
    parser.add_argument("--evidence-root", type=Path, required=True)
    args = parser.parse_args()
    result = verify(
        args.package,
        args.receipt,
        args.expectation,
        args.producer_root,
        args.evidence_root,
    )
    sys.stdout.write(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False) + "\n")
    if result["classification"] == "invalid_receipt":
        return 2
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(_cli())
