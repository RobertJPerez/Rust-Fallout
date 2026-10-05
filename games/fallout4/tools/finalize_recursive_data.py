#!/usr/bin/env python3
"""Validate and finalize a completed fingerprint pass under ignored local/."""
from __future__ import annotations

import hashlib
import json
import os
import sys
from collections import Counter
from pathlib import Path, PurePosixPath
from typing import Any


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require_local(path: Path, local_root: Path) -> Path:
    if path.is_absolute() or ".." in path.parts or not path.parts or path.parts[0].casefold() != "local":
        raise ValueError("evidence paths must be relative paths under ignored local/")
    resolved = path.resolve(strict=True)
    resolved.relative_to(local_root)
    return resolved


def finalize(install_root: Path, census_path: Path, evidence_dir: Path) -> dict[str, Any]:
    local_root = Path("local").resolve(strict=True)
    census_path = require_local(census_path, local_root)
    evidence_dir = require_local(evidence_dir, local_root)
    if (evidence_dir / "manifest.json").exists() or (evidence_dir / "complete.json").exists():
        raise ValueError("evidence directory already has a completion report")
    data_root = install_root.resolve(strict=True) / "Data"
    census_bytes = census_path.read_bytes()
    census_hash = hashlib.sha256(census_bytes).hexdigest()
    census = json.loads(census_bytes)
    baseline = {
        PurePosixPath(row["path"]).relative_to("Data").as_posix().casefold(): row
        for row in census["files"]
        if row["path"].casefold().startswith("data/")
    }
    rows_path = evidence_dir / "files.jsonl"
    rows = [json.loads(line) for line in rows_path.read_text(encoding="utf-8").splitlines() if line]
    if not rows:
        raise ValueError("fingerprint ledger is empty")

    rows_by_path: dict[str, dict[str, Any]] = {}
    extension_counts: Counter[str] = Counter()
    total_bytes = 0
    nested_bytes = 0
    prior_matches = 0
    nested_rows: list[tuple[dict[str, Any], Path]] = []
    for row in rows:
        relative = PurePosixPath(row["relative_path"])
        if relative.is_absolute() or ".." in relative.parts or not relative.parts:
            raise ValueError(f"unsafe path in fingerprint ledger: {relative}")
        key = relative.as_posix().casefold()
        if key in rows_by_path:
            raise ValueError(f"duplicate path in fingerprint ledger: {relative}")
        rows_by_path[key] = row
        nested = len(relative.parts) > 1
        if row["nested"] != nested:
            raise ValueError(f"nested-path marker disagrees for {relative}")
        if row["sha256_before"] != row["sha256_after"]:
            raise ValueError(f"source fingerprint changed during scan: {relative}")
        if len(row["sha256_before"]) != 64 or row["bytes"] < 0:
            raise ValueError(f"invalid fingerprint metadata for {relative}")
        expected = baseline.get(key)
        if expected is None:
            if row["matched_prior_top_level_census"]:
                raise ValueError(f"uncensused path claims a prior census match: {relative}")
        else:
            if not row["matched_prior_top_level_census"]:
                raise ValueError(f"censused path is marked as new: {relative}")
            if expected["bytes"] != row["bytes"] or expected["sha256"] != row["sha256_before"]:
                raise ValueError(f"prior top-level fingerprint disagreement for {relative}")
            prior_matches += 1
        total_bytes += row["bytes"]
        extension_counts[relative.suffix.casefold()] += 1
        if nested:
            nested_bytes += row["bytes"]
            nested_rows.append((row, data_root.joinpath(*relative.parts)))

    if prior_matches != len(baseline):
        raise ValueError(f"matched {prior_matches} prior Data files; expected {len(baseline)}")
    if len(rows_by_path) != len(rows):
        raise ValueError("fingerprint ledger has duplicate paths")

    actual_paths: set[str] = set()
    for directory, child_dirs, files in os.walk(data_root, followlinks=False):
        current_dir = Path(directory)
        if any((current_dir / child).is_symlink() for child in child_dirs):
            raise ValueError("a Data directory is a reparse point; links are not followed")
        for filename in files:
            path = current_dir / filename
            if path.is_symlink():
                raise ValueError("a Data file is a reparse point")
            actual_paths.add(path.relative_to(data_root).as_posix().casefold())
    if actual_paths != set(rows_by_path):
        missing = sorted(actual_paths - set(rows_by_path))
        stale = sorted(set(rows_by_path) - actual_paths)
        raise ValueError(f"current Data file set differs from ledger; unlisted={missing[:5]}, absent={stale[:5]}")
    for row in rows:
        relative = PurePosixPath(row["relative_path"])
        path = data_root.joinpath(*relative.parts)
        if path.stat().st_size != row["bytes"]:
            raise ValueError(f"source byte length changed after scan: {relative}")
    for row, path in nested_rows:
        if sha256_file(path) != row["sha256_after"]:
            raise ValueError(f"nested source fingerprint changed after scan: {row['relative_path']}")

    file_list_hash = sha256_file(rows_path)
    extension_json = dict(sorted(extension_counts.items()))
    manifest = {
        "schema": 1,
        "status": "verified-recursive-physical-data-tree-not-active-profile",
        "installation_data_root": str(data_root),
        "source_census": "local/proof-fo4-002/census.json",
        "source_census_sha256": census_hash,
        "recursive_files": len(rows),
        "direct_files": sum(not row["nested"] for row in rows),
        "nested_files": len(nested_rows),
        "nested_bytes": nested_bytes,
        "total_bytes": total_bytes,
        "prior_top_level_census_files_matched": prior_matches,
        "prior_top_level_census_files_expected": len(baseline),
        "new_recursive_files_not_in_top_level_census": len(rows) - prior_matches,
        "extension_counts": extension_json,
        "recursive_file_inventory_sha256": file_list_hash,
        "nested_files_rehashed_after_scan": len(nested_rows),
        "all_file_fingerprints_stable": True,
        "retail_files_modified": False,
        "runtime_ready": False,
        "limits": [
            "This is the physical Data tree only; VFS deployment and active profiles are not observed.",
            "The frozen top-level census fingerprints match; nested files were fingerprinted before and after and rehashed at finalization.",
            "File presence does not establish plugin activation, asset use, archive precedence or gameplay compatibility.",
        ],
    }
    manifest_path = evidence_dir / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    completion = {
        "schema": 1,
        "manifest_sha256": sha256_file(manifest_path),
        "files_jsonl_sha256": file_list_hash,
        "files": len(rows),
        "nested_files": len(nested_rows),
        "nested_rehashed_after_scan": len(nested_rows),
        "all_file_fingerprints_stable": True,
        "complete": True,
        "runtime_ready": False,
    }
    (evidence_dir / "complete.json").write_text(json.dumps(completion, indent=2) + "\n", encoding="utf-8")
    return completion


def main() -> int:
    if len(sys.argv) != 4:
        print("usage: py -3.13 tools/finalize_recursive_data.py <Fallout-4-root> local/<census-dir>/census.json local/<evidence-dir>", file=sys.stderr)
        return 2
    try:
        result = finalize(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))
        print(json.dumps(result, indent=2))
        return 0
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"recursive Data finalization failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
