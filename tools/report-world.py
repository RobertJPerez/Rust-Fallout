"""Publish metadata from completed world/NIF probes. Does not run or bless gameplay."""
import hashlib
import json
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL = ROOT / "local"


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


cell = read(LOCAL / "cell-docmitchell-final.json")
census = read(LOCAL / "nif-census-expanded.json")
comparison = read(LOCAL / "nif-comparison-docmitchell-final.json")
variants = read(LOCAL / "nif-comparison-variants.json")
negative = read(LOCAL / "nif-deliberate-mismatch-report.json")
profile_hash = hashlib.sha256((ROOT / "profiles/manifest.json").read_bytes()).hexdigest()
blocks, block_files, streams = Counter(), Counter(), Counter()
failures = []
for archive in census["archives"]:
    blocks.update(archive["blocks"])
    block_files.update(archive["files_with_block"])
    streams.update(archive["stream_versions"])
    for failure in archive["failures"]:
        failures.append({"archive": Path(archive["archive"]).name, "archive_sha256": archive["archive_sha256"], **failure})
coverage = {
    "schema_version": 1, "profile": "nv-original", "profile_manifest_sha256": profile_hash,
    "source_report": "local/nif-census-expanded.json", "archives_scanned": len(census["archives"]),
    **{key: sum(a[key] for a in census["archives"]) for key in ("nif_members", "kf_members", "containers_decoded", "decoded_bytes")},
    "bethesda_stream_versions": dict(sorted(streams.items(), key=lambda v: int(v[0]))),
    "total_blocks": sum(blocks.values()), "unique_block_types": len(blocks),
    "block_types": [{"name": name, "occurrences": count, "files": block_files[name],
                     "container_status": "decoded", "payload_semantics": "unknown"} for name, count in sorted(blocks.items())],
    "failures": failures, "all_failures_are_unsupported_20_0_0_4": all('20.0.0.4' in f['reason'] for f in failures),
    "independent_comparison": {"cell_files": comparison["files_compared"], "cell_blocks": comparison["blocks_compared"],
        "older_stream_samples": variants["files_compared"], "older_stream_blocks": variants["blocks_compared"],
        "all_equal": comparison["all_equal"] and variants["all_equal"],
        "deliberate_block_size_mismatch_rejected": not negative["all_equal"],
        "scope": comparison["comparison"]},
    "geometry_animation_collision_accepted": False,
}
write(ROOT / "parity/nif-coverage.json", coverage)
summary = {
    "schema_version": 1, "profile_manifest_sha256": profile_hash,
    "cell": cell["key"], "editor_id": bytes(cell["editor_id"]).decode("ascii"),
    "source_plugin": cell["source_plugin"], "record_offset": cell["record_offset"],
    "interior": bool(cell["cell"]["flags"]["value"] & 1),
    "placed_references": len(cell["references"]),
    "record_kinds": dict(Counter(r["record_kind"] for r in cell["references"])),
    "base_link_statuses": dict(Counter(r["base"]["status"] for r in cell["references"] if r["base"])),
    "door_links": sum(r["teleport_door"] is not None for r in cell["references"]),
    "link_failures": cell["link_failures"], "integrity_failures": cell["integrity_failures"],
    "distinct_base_records": len(cell["models"]), "model_statuses": dict(Counter(m["status"] for m in cell["models"])),
    "distinct_model_paths_probed": len(cell["model_probes"]),
    "model_bytes_decoded": sum(m["decoded_bytes"] or 0 for m in cell["model_probes"]),
    "model_failures": sum(m["error"] is not None for m in cell["model_probes"]),
    "model_cache_entries_reused": sum(m["cache"]["reused"] for m in cell["model_probes"] if m["cache"]),
    "nif_block_entries_compared": comparison["blocks_compared"], "nif_oracle_all_equal": comparison["all_equal"],
    "other_child_records": cell["other_child_records"], "runtime_ready": False, "unknown": cell["unknown"],
    "raw_reports": ["local/cell-docmitchell-final.json", "local/nif-comparison-docmitchell-final.json", "local/nif-comparison-variants.json"],
}
write(ROOT / "reports/world-inspection.json", summary)
ledger_path = ROOT / "parity/requirements.json"
ledger = read(ledger_path)
additions = [
    ("fnv.content.indexed_reads", "content", "unit-tested", "Indexed reads preserve raw bodies and reject changed headers.",
     ["indexed_reads_match_streamed_payloads_and_reject_stale_headers"], ["Index is rebuilt at startup; no serialized index cache yet"]),
    ("fnv.world.cell_dependencies", "world", "unit-tested", "Resolve winning cell membership, placed transforms, and typed base/enable-parent/door links.",
     ["cell_membership_uses_winning_parent_and_does_not_resurrect_deleted_references", "placed_field_validation_rejects_duplicate_truncated_and_nonfinite_transforms", "present_but_wrong_kind_targets_do_not_count_as_resolved"],
     ["No retail per-field export comparison", "No activation, movement, enable-parent evaluation or teleport execution"]),
    ("fnv.nif.container", "formats", "oracle-tested", "Read bounded NIF/KF container tables and compare version, block type/index/size and roots with nifly.",
     ["inventories_unknown_blocks_without_discarding_their_bytes", "all_truncated_prefixes_and_bad_roots_fail", "version_dispatch_and_count_budgets_are_enforced", "observed_nv_stream_revisions_keep_their_identity", "tools/compare-nif.py"],
     ["Six legacy 20.0.0.4 files unsupported", "Oracle sample is 218 files, not every NIF/KF payload", "Container evidence alone does not establish payload semantics; see the separate NIF scene ledger", "No rendering, animation or physics parity"]),
]
ids = {row[0] for row in additions}
ledger["requirements"] = [r for r in ledger["requirements"] if r["id"] not in ids]
for identifier, subsystem, status, requirement, tests, gaps in additions:
    ledger["requirements"].append({
        "id": identifier, "game_profile": profile_hash, "subsystem": subsystem, "requirement": requirement,
        "evidence": ["reports/world-inspection.json", "parity/nif-coverage.json", "sources.lock.json"],
        "affected_content": cell["key"] if subsystem == "world" else "NV official corpus; see coverage denominators",
        "status": status, "tests": tests, "known_gaps": gaps, "tolerance": "Exact bytes/identities/counts; source transforms remain unconverted",
        "last_verified_revision": None,
    })
write(ledger_path, ledger)
print(f"Published cell/NIF evidence: {summary['placed_references']} references, {coverage['containers_decoded']} containers decoded.")
