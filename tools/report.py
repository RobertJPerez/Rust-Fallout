"""Summarize this checkpoint's local evidence without copying game content.

This does not execute tests or turn observations into acceptance. Keep raw reports
under local/; the public reports contain counts, hashes, offsets, and open gates.
"""
import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def summarize(local):
    baseline = read(local / "baseline.json")
    census = read(local / "census-with-scripts.json")
    archives = read(local / "archive-comparison.json")
    oracle = {p["name"]: p for p in read(local / "plugin-oracle.json")}
    resolution = read(local / "resolution-final.json")
    plan = read(local / "import-plan.json")
    plugins = census["plugins"]
    records = Counter()
    extensions = Counter()
    conditions = {}
    comparisons = []
    for plugin in plugins:
        name = plugin["name"]
        independent = oracle[name]
        equal = (
            plugin["records_excluding_header"] == independent["parsed_record_ids"]
            and plugin["declared_records_and_groups"] == independent["declared_records_and_groups"]
            and plugin["records_excluding_header"] + plugin["groups"] == independent["declared_records_and_groups"]
            and plugin["masters"] == independent["masters"]
            and abs(plugin["header_version"] - independent["header_version"]) < 0.000001
        )
        comparisons.append({
            "plugin": name, "records": plugin["records_excluding_header"],
            "groups": plugin["groups"], "counts_masters_header_version_equal": equal,
            "integrity_failures": len(plugin["integrity_issues"]),
        })
        records.update({k: v["occurrences"] for k, v in plugin["record_kinds"].items()})
        for function, usage in plugin["scripts"]["condition_functions"].items():
            item = conditions.setdefault(function, {
                "function_id": int(function), "name": None, "status": "decoded",
                "behavior_status": "unknown", "observed_calls": 0, "plugins": [], "examples": [],
            })
            item["observed_calls"] += usage["occurrences"]
            item["plugins"].append(name)
            item["examples"].extend({"plugin": name, **example} for example in usage["examples"][:1])
    for archive in census["archives"]:
        extensions.update(archive["extensions"])
    profile = {
        "schema_version": 1,
        "profiles": [{
            "id": "nv-original", "data_status": "present-with-format-exceptions",
            "content_fingerprint": baseline["content_fingerprint"],
            "executable_version": "1.4.0.525", "formats_observed": ["ESM HEDR 1.32/1.33/1.34", "BSA 104", "NIF/KF 20.2.0.7, user 11", "NIF/KF 20.0.0.4: unsupported"],
            "inputs": [f for f in baseline["files"] if f["path"].lower().endswith((".esm", ".esp", ".bsa")) or f["path"] == "FalloutNV.exe"],
            "inspection_order": resolution["explicit_load_order"], "retail_active_order_verified": False,
            "effective_language": None, "effective_ini": None, "difficulty": None, "input_bindings": None,
            "oracles": ["ba2", "esplugin", "nifly standalone tool", "xEdit source; no executable export yet"],
            "supported_scenarios": ["headless content inspection", "single asset decoding/cache", "preparation dry run", "typed CELL/placement inspection", "NIF/KF container census"],
            "accepted_gameplay_scenarios": [], "save_format": "not implemented",
        }] + [{
            "id": name, "data_status": "not-baselined", "content_fingerprint": None,
            "formats_to_investigate": formats, "rules": "separate profile; no NV semantic inheritance assumed",
            "oracles": oracles, "supported_scenarios": [], "save_format": "not implemented",
        } for name, formats, oracles in [
            ("fo3-original", ["FO3 ESM/ESP", "BSA", "NIF/KF", "ObScript"], ["retail FO3", "xEdit"]),
            ("ttw-compatible", ["TTW-generated NV plugins/assets", "ObScript and extender dependencies"], ["pinned TTW installation"]),
            ("fo4-original", ["FO4 ESM/ESP/ESL", "BA2", "NIF", "PEX", "HKX", "BGSM/BGEM"], ["retail FO4", "xEdit"]),
            ("fo76-research", ["versioned FO76 client records/archives", "client/server dependencies"], ["authorized observations and client data"]),
            ("starfield-probe", ["Starfield records/BA2", "mesh/material/animation formats"], ["retail bounded probes", "xEdit"]),
            ("unified-crossover", ["versioned canonical content and campaign-specific extensions"], ["explicit crossover policy; undecided"]),
        ]],
    }
    profile_path = ROOT / "profiles/manifest.json"
    write(profile_path, profile)
    profile_hash = hashlib.sha256(profile_path.read_bytes()).hexdigest()
    shared = {"schema_version": 1, "profile": "nv-original", "profile_manifest_sha256": profile_hash,
              "last_verified_revision": None, "revision_note": "Uncommitted checkpoint; no engine commit exists yet."}
    requirements = []
    def requirement(identifier, subsystem, status, behavior, tests, gaps, evidence):
        requirements.append({
            "id": identifier, "game_profile": profile_hash, "subsystem": subsystem,
            "requirement": behavior, "evidence": evidence, "affected_content": "See content-coverage.json and local provenance reports",
            "status": status, "tests": tests, "known_gaps": gaps,
            "tolerance": "Exact for bytes/counts/identities; no gameplay tolerances established",
            "last_verified_revision": None,
        })
    requirement("fnv.plugin.bounds", "formats", "unit-tested", "Reject invalid parent extents, compressed sizes, checksums, depth, and truncated fields.",
                ["truncation_and_bad_parent_bounds_return_errors", "rejects_zlib_size_lies_trailing_bytes_and_checksum_damage", "group_depth_limit_is_enforced_without_recursion", "adversarial_small_inputs_never_panic"],
                ["Mutation sweep is not sustained fuzzing", "Known vanilla LAND requires a scoped compatibility decision"], ["crates/fallout-data/tests/framing.rs", "docs/format-exceptions.md"])
    requirement("fnv.plugin.census", "formats", "oracle-tested", "Official corpus record counts and master lists agree with independent esplugin.",
                ["tools/plugin-oracle", "tools/report.py"], ["Compressed bodies are not checked by esplugin", "Typed field reference exports still needed"], ["reports/corpus.json", "sources.lock.json"])
    requirement("fnv.archive.payloads", "archives", "oracle-tested", "Compare every member path/hash and decoded payload against ba2.",
                ["tools/archive-compare --all"], ["Two malformed text payloads disagree", "Only local BSA v104 corpus tested"], ["reports/corpus.json", "docs/format-exceptions.md"])
    requirement("fnv.content.identity", "content", "unit-tested", "Resolve stable origin/local identities and explicit structural override chains.",
                ["identity_survives_rebasing_and_keeps_campaigns_separate", "explicit_override_order_preserves_deletion_and_rejects_missing_masters", "conflicting_noncanonical_self_ids_are_rejected"],
                ["Record-specific merge exceptions and field links unimplemented"], ["crates/fallout-data/src/identity.rs", "sources.lock.json"])
    requirement("fnv.script.references", "content", "unit-tested", "Two-pass SCRO links allow forward cycles and distinguish the NV player runtime dependency.",
                ["two_pass_script_links_allow_forward_cycles_and_report_missing_targets", "player_reference_is_an_explicit_nv_runtime_dependency"],
                ["No player object or script VM exists", "No independent per-reference retail export"], ["parity/content-coverage.json", "docs/format-exceptions.md"])
    requirement("fnv.vfs.paths", "vfs", "unit-tested", "Normalize safe legacy paths while retaining original bytes and ambiguous candidates.",
                ["normalizes_ascii_without_destroying_legacy_bytes", "ambiguous_mount_never_silently_wins"],
                ["Loose-file mounting and archive/invalidation precedence unimplemented"], ["crates/fallout-data/src/vfs.rs"])
    requirement("fnv.conditions.inventory", "scripting", "decoded", "Inventory CTDA function IDs with caller offsets independently of SCDA opcodes.",
                ["condition_inventory_keeps_function_ids_separate_from_script_opcodes"], ["No condition evaluation", "Names/parameter types not yet audited"], ["parity/conditions.json", "sources.lock.json"])
    requirement("r3.profile.identity", "profiles", "unit-tested", "Form and cache identities include the game profile.",
                ["identity_survives_rebasing_and_keeps_campaigns_separate", "profiles_and_transform_revisions_cannot_share_a_cache_identity"],
                ["Save loading and cross-profile rejection are unimplemented"], ["profiles/manifest.json", "docs/profiles.md"])
    requirement("r3.preparation.plan", "import", "unit-tested", "Preparation identities are independent of discovery order and include master digests.",
                ["enumeration_order_does_not_change_the_plan", "master_changes_invalidate_only_dependent_jobs"],
                ["Only source indexing jobs planned", "Typed per-asset conversions and dependency graphs incomplete"], ["local/import-plan.json", "crates/fallout-data/src/planning.rs"])
    requirement("r3.asset.publication", "import", "unit-tested", "Publish a verified blob before its manifest; safely resume interrupted publication and reject corrupt reuse.",
                ["interrupted_publication_resumes_and_corrupt_reuse_fails", "cannot_publish_inside_source_tree"],
                ["No full job scheduler/cancellation/journal", "Failure injection is in-process, not OS termination", "No save-grade durability claim"], ["local/asset-first.json", "local/asset-reuse.json", "crates/fallout-data/src/cache.rs"])
    for identifier, subsystem, behavior in [
        ("fnv.vfs.precedence", "vfs", "Match effective retail loose/archive lookup and invalidation rules."),
        ("fnv.script.execution", "scripting", "Execute source-less ObScript with typed native calls, budgets, and state traces."),
        ("fnv.conditions.execution", "scripting", "Match condition parameters, targets, evaluation order, and results."),
        ("fnv.world.interior", "world", "Load and move through a real interior with measured rendering and collision."),
        ("fnv.world.goodsprings", "world", "Stream Goodsprings using actual records, geometry, terrain, materials, and collision."),
        ("fnv.persistence", "persistence", "Save and restore authoritative state, rejecting incompatible profiles."),
        ("fnv.campaign", "gameplay", "Complete required base-game and DLC routes with retail state comparisons."),
        ("ttw.runtime.dependencies", "ttw", "Census and implement a pinned TTW installation's required providers."),
        ("fo3.original", "other-games", "Reproduce original FO3 under its own profile."),
        ("fo4.original", "other-games", "Reproduce FO4 with its format and runtime adapters."),
        ("fo76.research", "other-games", "Separate observed client behavior from unknown server responsibilities."),
        ("crossover.travel", "travel", "Preserve per-campaign identity/state through transactional round trips."),
    ]:
        requirement(identifier, subsystem, "unknown", behavior, [], ["Unimplemented; no passing acceptance scenario"], ["docs/references/Fallout_Rust_Codex_Master_Brief.txt", "NEXT_STEPS.md"])
    ledger_path = ROOT / "parity/requirements.json"
    if ledger_path.exists():
        regenerated = {item["id"] for item in requirements}
        requirements.extend(item for item in read(ledger_path)["requirements"] if item["id"] not in regenerated)
    write(ledger_path, {**shared, "accepted_scenarios": [], "requirements": requirements})
    write(ROOT / "parity/conditions.json", {
        **shared, "evidence": "https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas",
        "interpretation": "CTDA function IDs, not SCDA bytecode opcodes. Names and execution semantics remain unaudited.",
        "unique_functions_observed": len(conditions), "functions": sorted(conditions.values(), key=lambda v: v["function_id"]),
    })
    script_counts = {key: sum(p["scripts"][key] for p in plugins) for key in (
        "headers", "compiled_bodies", "compiled_bytes", "source_text_fields", "explicit_form_references", "local_variable_references")}
    write(ROOT / "parity/commands.json", {
        **shared, "status": "unknown", "script_metadata": script_counts,
        "opcode_decoder_implemented": False, "unique_command_denominator": None, "commands": [],
        "remaining": "Decode NV SCDA instruction/event framing and argument boundaries before enumerating commands. No VM or native command execution exists.",
    })
    failures = []
    for archive in archives:
        for failure in archive["failures"]:
            failures.append({"archive": Path(archive["archive"]).name, **failure})
    links = resolution["script_links"]
    write(ROOT / "parity/content-coverage.json", {
        **shared, "content_fingerprint": baseline["content_fingerprint"],
        "plugins_scanned": len(plugins), "archives_indexed": len(census["archives"]),
        "records_excluding_tes4_headers": sum(p["records_excluding_header"] for p in plugins),
        "record_kinds_including_tes4": dict(sorted(records.items())),
        "archive_members": sum(a["entries"] for a in archives), "asset_extensions": dict(sorted(extensions.items())),
        "archive_payloads_equal": sum(a["payloads_compared"] for a in archives),
        "archive_decode_failures": len(failures), "cross_archive_path_collisions": census["cross_archive_path_collisions"],
        "unique_definition_keys": resolution["unique_definitions"], "overridden_definition_keys": resolution["overridden_definitions"],
        "script_metadata": script_counts, "condition_function_ids": len(conditions),
        "condition_occurrences": sum(c["observed_calls"] for c in conditions.values()),
        "scro_links": links, "integrity_failures": census["integrity_failures"],
        "accepted_scenarios": [], "campaign_routes_verified": 0,
        "additional_coverage": ["reports/world-inspection.json", "parity/nif-coverage.json"],
        "unknown": ["typed field links outside the selected cell dependency path", "complete model/texture/audio dependency closure", "SCDA commands and events",
                    "NIF block-internal geometry/animation/collision semantics", "UI templates/operators", "audio codecs", "all gameplay semantics"],
    })
    write(ROOT / "reports/corpus.json", {
        **shared, "source_files_hashed": len(baseline["files"]), "source_bytes_hashed": sum(f["bytes"] for f in baseline["files"]),
        "content_fingerprint": baseline["content_fingerprint"], "plugin_comparisons": comparisons,
        "all_plugin_comparisons_equal": all(p["counts_masters_header_version_equal"] for p in comparisons),
        "archive_comparisons": [{**a, "archive": Path(a["archive"]).name} for a in archives],
        "decoded_bytes_compared": sum(a["decoded_bytes_compared"] for a in archives),
        "plugin_integrity_issues": [{"plugin": p["name"], "issues": p["integrity_issues"]} for p in plugins if p["integrity_issues"]],
        "plan": {"jobs": len(plan["jobs"]), "unsupported_inputs": len(plan["unsupported"]), "runtime_ready": plan["runtime_ready"]},
        "accepted": False,
    })
    metadata = read(local / "cargo-metadata.json")
    write(ROOT / "reports/dependency-licenses.json", {
        "scope": "Workspace Cargo metadata, including offline archive oracle; GPL plugin oracle is a separate workspace.",
        "limitations": "Manifest declarations, not a completed per-file/transitive source audit.",
        "packages": [{k: p.get(k) for k in ("name", "version", "source", "license", "license_file", "repository")}
                     for p in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"]))],
    })
    print(f"Wrote corpus and parity metadata for {len(plugins)} plugins, {len(archives)} archives, {len(conditions)} condition IDs.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local", type=Path, default=ROOT / "local")
    summarize(parser.parse_args().local)
