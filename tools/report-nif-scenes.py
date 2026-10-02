"""Publish scene-decoder evidence from completed local runs; never grants gameplay acceptance."""
from collections import Counter
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL = ROOT / "local"


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def main():
    census_path = LOCAL / "nif-scene-census-01.json"
    census = read(census_path)
    interior = read(LOCAL / "nif-scene-comparison-docmitchell-verified.json")
    variants = read(LOCAL / "nif-scene-comparison-variants-final.json")
    negative = read(LOCAL / "nif-scene-negative-checks.json")
    diagnostics = read(LOCAL / "nif-oracle-rejected-diagnostics.json")
    rejected = read(LOCAL / "nif-rejected-probes.json")
    baseline = read(LOCAL / "baseline.json")
    baseline_archives = {Path(row["path"]).name: row["sha256"] for row in baseline["files"] if row["path"].lower().endswith(".bsa")}
    archive_hashes_equal = all(baseline_archives.get(Path(a["archive"]).name) == a["archive_sha256"] for a in census["archives"])
    if not (interior["all_equal"] and variants["all_equal"] and negative["all_rejected"] and archive_hashes_equal):
        raise ValueError("Completed comparisons and unchanged archive identities are required")
    by_file = {row["file"]: row for row in diagnostics}
    if len(by_file) != len(diagnostics):
        raise ValueError("duplicate diagnostic input")
    rejected_evidence = []
    issue_fields = Counter()
    for row in rejected:
        filename = row["cache"]["key"] + ".blob"
        oracle = by_file.pop(filename)
        if digest(LOCAL / "nif-rejected-models" / filename) != row["sha256"]:
            raise ValueError("diagnostic source changed")
        issues = oracle["raw_scene_issues"]
        if oracle["load_code"] or not issues:
            raise ValueError("independent diagnostic did not confirm the rejected input")
        fields = Counter(issue["field"] for issue in issues)
        issue_fields.update(fields)
        rejected_evidence.append({"archive": Path(row["archive"]).name, "path": row["path"],
            "sha256": row["sha256"], "decoded_bytes": row["decoded_bytes"],
            "rust_failure": row["scene_failure"], "oracle_issues": len(issues), "oracle_fields": dict(fields),
            "oracle_first_issue": issues[0]})
    if by_file or len(rejected_evidence) != census["scene_failures"]:
        raise ValueError("incomplete diagnostic coverage")
    scenes = [archive["scene_payloads"] for archive in census["archives"]]
    counters = ["files_decoded", "objects", "meshes", "vertices", "triangles", "strip_degenerate_triangles",
                "unsupported_scene_edges", "missing_geometry_arrays", "source_triangle_count_mismatches"]
    totals = {key: sum(scene[key] for scene in scenes) for key in counters}
    decoded, unsupported, oracle_blocks = Counter(), Counter(), Counter()
    for scene in scenes:
        decoded.update(scene["decoded_blocks"])
        unsupported.update(scene["unsupported_blocks"])
    for directory in ["scenes-docmitchell-final", "scenes-variants-final"]:
        manifest = read(LOCAL / directory / "manifest.json")
        for probe in manifest["results"]:
            report = read(LOCAL / directory / probe["report"])
            index = report["index"]
            for block in report["scene"]["objects"] + report["scene"]["meshes"]:
                oracle_blocks[index["block_types"][index["blocks"][block["block"]]["type_index"]]] += 1
    profile_hash = digest(ROOT / "profiles/manifest.json")
    summary = {
        "schema_version": 1, "profile": "nv-original", "profile_manifest_sha256": profile_hash,
        "archive_count": len(census["archives"]), "archive_hashes_match_original_baseline": archive_hashes_equal,
        "container_failures": census["failures"], "scene_failures": census["scene_failures"],
        "successful_scene_files": totals,
        "independent_comparison": {
            "files": interior["files"] + variants["files"], "all_equal": True,
            **{key: sum(row[key] for report in [interior, variants] for row in report["results"]) for key in ["objects", "meshes", "vertices", "triangles"]},
            "max_world_transform_absolute_error": max(row["max_world_transform_absolute_error"] for report in [interior, variants] for row in report["results"]),
            "tolerance": interior["tolerance"], "scope": interior["scope"], "oracle": interior["oracle"],
        },
        "negative_checks": negative,
        "rejected_source_diagnostics": {"files_confirmed": len(rejected_evidence), "field_components": dict(issue_fields), "files": rejected_evidence},
        "decoded_block_types": dict(decoded), "unsupported_block_types": dict(unsupported),
        "raw_reports": {str(path.relative_to(ROOT)).replace("\\", "/"): digest(path) for path in [census_path,
            LOCAL / "nif-scene-comparison-docmitchell-verified.json", LOCAL / "nif-scene-comparison-variants-final.json",
            LOCAL / "nif-oracle-rejected-diagnostics.json", LOCAL / "nif-scene-negative-checks.json"]},
        "runtime_ready": False, "accepted_scenarios": [],
        "known_gaps": ["Only six block payload types implemented; unknown bytes remain indexed", "25 strict payload failures and six legacy container failures",
            "No shader/texture payloads, lighting, skinning, controller evaluation, collision or rendering", "Game/renderer axes and units not visually verified",
            "Property/controller/skin/collision reference target types not fully validated", "Retail behavior of rejected data is not established"],
    }
    write(ROOT / "reports/nif-scenes.json", summary)
    write(ROOT / "parity/nif-scene-coverage.json", {
        "schema_version": 1, "profile_manifest_sha256": profile_hash, "scope": "counts only in files whose supported payload decoding completed",
        "block_types": [{"name": name, "decoded_occurrences": count, "oracle_compared_occurrences": oracle_blocks[name], "status": "oracle-tested"} for name, count in sorted(decoded.items())],
        "unsupported_types": dict(sorted(unsupported.items())), "accepted": False,
    })
    # The container generator owns container facts. Add the separate payload result
    # after it runs; do not imply the whole corpus had an independent comparison.
    coverage_path = ROOT / "parity/nif-coverage.json"
    coverage = read(coverage_path)
    for block in coverage["block_types"]:
        if block["name"] in decoded:
            block["payload_semantics"] = "oracle-tested on samples; see nif-scene-coverage.json for scope"
    write(coverage_path, coverage)
    ledger_path = ROOT / "parity/requirements.json"
    ledger = read(ledger_path)
    identifier = "fnv.nif.scene_mesh_payloads"
    ledger["requirements"] = [row for row in ledger["requirements"] if row["id"] != identifier]
    ledger["requirements"].append({"id": identifier, "game_profile": profile_hash, "subsystem": "formats",
        "requirement": "Decode bounded scene graphs and source triangle payloads, preserving unknown blocks and unsupported edges",
        "status": "oracle-tested", "affected_content": "NV archived models; Doc Mitchell interior and older-stream samples",
        "evidence": ["reports/nif-scenes.json", "parity/nif-scene-coverage.json", "docs/nif-scenes.md"],
        "tests": ["crates/fallout-data/tests/nif_scene.rs", "tools/compare-nif-scenes.py", "tools/check-nif-scene-comparison.py"],
        "known_gaps": summary["known_gaps"], "tolerance": interior["tolerance"], "last_verified_revision": None})
    write(ledger_path, ledger)
    print(f"Published {totals['files_decoded']} decoded scenes, {len(rejected_evidence)} confirmed rejected sources, {summary['independent_comparison']['files']} exact comparisons")


if __name__ == "__main__":
    main()
