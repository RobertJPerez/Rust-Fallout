"""Fresh checkpoint-45 source integration proof; historical 44 defaults are unchanged."""
import argparse
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from integration_proof import (FOREIGN, actor_findings, binary_identity, digest, document,
                               frozen_binaries, frozen_inputs, frozen_research, git,
                               long_path, new_local_path, rejection, require, skin_report,
                               snapshot, write_json)
from integration_actor_checks import authored_actor_checks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path("."))
    parser.add_argument("--install", type=Path, required=True)
    parser.add_argument("--build-receipt", type=Path, required=True)
    parser.add_argument("--run-directory", type=Path, required=True)
    parser.add_argument("--regression-directory", type=Path, required=True)
    args = parser.parse_args()
    root, install = args.repository.resolve(), args.install.resolve()
    run = new_local_path(root, args.run_directory)
    regression = new_local_path(root, args.regression_directory)
    require(run != regression, "Lane and regression evidence directories must differ")
    require(not git(root, "status", "--porcelain"), "Commit candidate tooling before proof")
    revision = git(root, "rev-parse", "HEAD").decode().strip()
    before = snapshot(root, revision)
    binaries, inputs, research = frozen_binaries(root), frozen_inputs(root), frozen_research(root)
    build_path = (root / args.build_receipt).resolve()
    require(build_path.is_relative_to((root / "local").resolve()), "Build receipt must be private to this worktree")
    build = document(build_path)
    require(build["revision"] == revision and build["source_snapshot_sha256"] == before["sha256"]
            and build["source_unchanged"] is True, "Build receipt belongs to another candidate")
    for name, expected in build["binaries"].items():
        require(binaries.get(name) == expected, f"Built candidate binary differs: {name}")
    required_builds = {"workspace-check", "source-lane-guards", "integration-guards", "release-cli",
                       "actor-oracle", "skin-oracle", "operand-oracle"}
    require({command["name"] for command in build["commands"]} == required_builds,
            "Candidate build/check coverage differs")
    inputs[build_path.relative_to(root).as_posix()] = digest(build_path)
    for command in build["commands"]:
        require(command["exit_code"] == 0, "Candidate build has a failed command")
        path = (root / command["log"]).resolve()
        require(path.is_relative_to((root / "local").resolve()), "Build log escapes private worktree")
        require(digest(path) == command["log_sha256"], "Candidate build log differs")
        inputs[path.relative_to(root).as_posix()] = command["log_sha256"]
    cli, runner = root / "target/release/fallout.exe", root / "target/release/fallout-evidence.exe"
    actor = root / "local/actor-oracle-build/Release/actor-oracle.exe"
    skin = root / "local/nif-skin-oracle-build/Release/nif-skin-oracle.exe"
    operand = root / "local/operand-oracle-build/Release/operand-oracle.exe"
    order = root / "profiles/nv-inspection-order.json"
    run.mkdir()
    write_json(run / "frozen-start.json", {"source": before, "binaries": binaries,
               "inputs": inputs, "research": research, "build_receipt": str(build_path.relative_to(root))})
    write_json(run / "source-snapshot.json", before)
    commands = []
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(root / "target")

    def execute(name, arguments, expected=0, diagnostic=None, output=None, env=None):
        arguments = [str(value) for value in arguments]
        print(f"Running {name}", flush=True)
        log = run / f"{name}.log"
        with log.open("xb") as errors:
            if output is None:
                result = subprocess.run(arguments, cwd=root, env=env or environment,
                                        stdout=errors, stderr=subprocess.STDOUT)
            else:
                with output.open("xb") as stdout:
                    result = subprocess.run(arguments, cwd=root, env=env or environment,
                                            stdout=stdout, stderr=errors)
        row = {"name": name, "arguments": arguments, "exit_code": result.returncode,
               "log_sha256": digest(log)}
        if output is not None:
            row["output_sha256"] = digest(output)
        commands.append(row)
        require(result.returncode == expected, f"{name} failed; see {log}")
        if diagnostic:
            require(diagnostic.encode() in log.read_bytes(), f"{name} failed for an unrelated reason")
            row["required_diagnostic"] = diagnostic

    powershell = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
    execute("source-lane-guards", [sys.executable, root / "tools/test_source_lane_guards.py"])
    execute("integration-guards", [sys.executable, root / "tools/test_integration_guards.py"])
    execute("actor-body-allocation-checks", [actor.with_name("actor-body-tests.exe")],
            diagnostic="actor body allocation-order checks: 6 passed")
    common_actor = [*powershell, root / "tools/actor-oracle/compare.ps1", "-Fallout", cli,
                    "-Oracle", actor, "-Install", install, "-LoadOrder", order]
    execute("actors-scalar", [*common_actor, "-RunDirectory", run / "actors-scalar"])
    modes = ["-IncludeAssociations", "-IncludeClasses", "-IncludeFactions", "-IncludePlacements", "-AllowSourceFindings"]
    execute("actors-combined", [*common_actor, "-RunDirectory", run / "actors", *modes])
    worker = document(run / "actors/worker-comparison.json")
    require(worker["started_source_revision"] == revision and worker["source_and_binaries_unchanged"] is True,
            "Actor receipt source differs")
    require(worker["engine_binary_sha256"] == binaries[cli.relative_to(root).as_posix()]
            and worker["oracle_binary_sha256"] == binaries[actor.relative_to(root).as_posix()],
            "Actor receipt binary differs")
    require([row["name"] for row in worker["phases"]] == ["cold", "warm", "reordered"], "Actor phases omitted")
    actor_reports = {}
    retained = None
    for phase in worker["phases"]:
        name = phase["name"]
        report_path = run / "actors" / f"{name}.json"
        require(digest(report_path) == phase["rust_report_sha256"], "Actor report identity differs")
        report = document(report_path)
        require(phase["exit_code"] == 1 and report["independent_comparison"]["equal"] is True,
                "Actor diagnostic exit was not an exact source comparison")
        finding = actor_findings(report)
        require(retained is None or retained == finding, "Actor phase changed the retained voice finding")
        retained = finding
        actor_reports[name] = report
    cold = actor_reports["cold"]
    warm = copy.copy(actor_reports["warm"])
    original = copy.copy(cold)
    original.pop("index_cache", None); warm.pop("index_cache", None)
    require(original == warm, "Cold/warm actor source facts differ")
    del original, warm
    native = document(run / "actors/oracle.json")
    actor_negatives = []
    for name, section, mutate in (
        ("scalar-fatigue", "definitions", lambda value: value["definitions"][0]["fields"]),
        ("association-omission", "actor_associations", lambda value: value["actor_associations"]["definitions"]),
        ("class-omission", "actor_classes", lambda value: value["actor_classes"]["definitions"]),
        ("faction-omission", "actor_factions", lambda value: value["actor_factions"]["definitions"]),
        ("placement-omission", "actor_placements", lambda value: value["actor_placements"]["definitions"]),
    ):
        altered = copy.deepcopy(native)
        rows = mutate(altered)
        require(rows, f"Actor negative fixture lacks {name}")
        if name == "scalar-fatigue":
            field = next(field for definition in altered["definitions"] for field in definition["fields"]
                         if field["value"].get("kind") == "configuration")
            field["value"]["fatigue"] ^= 1
        else:
            rows.pop()
        path = run / "actors" / f"altered-{name}.json"
        write_json(path, altered)
        reason = f"independent actor source comparison differs in {section}"
        execute(f"actor-negative-{name}", [cli, "actor-sources", "--install", install,
                "--load-order", order, "--include-associations", "--include-classes", "--include-factions",
                "--include-placements",
                "--compare-oracle", path, "--output", run / "actors" / f"rejected-{name}.json"],
                expected=1, diagnostic=reason)
        actor_negatives.append({"name": name, "diagnostic": reason})
        del altered
    del native, actor_reports
    actor_authored = authored_actor_checks(root, run, cli, actor, binaries, revision, execute, powershell)

    authored = {}
    for schema, script in ((1, "check_comparison.py"), (2, "check_partition_comparison.py"),
                           (3, "check_binding_comparison.py")):
        directory = run / f"skin-authored-{schema}"
        execute(f"skin-authored-{schema}", [sys.executable, root / "tools/nif-skin-oracle" / script,
                "--binary", cli, "--oracle", skin, "--output-dir", directory])
        summary = document(directory / "summary.json")
        binary_identity(document(directory / "oracle.json"), binaries[skin.relative_to(root).as_posix()])
        require(summary["cli_sha256"] == binaries[cli.relative_to(root).as_posix()], "Authored skin CLI differs")
        authored[str(schema)] = summary
    skin_diagnostics = []
    skin_headers = {"file_omission": "oracle file count differs from inspected inputs",
                    "duplicate_file": "duplicate oracle file name",
                    "normalization_contract": "oracle provenance or raw-field comparison contract is missing",
                    "prepare_data": "oracle provenance or raw-field comparison contract is missing",
                    "source_pin": "oracle provenance or raw-field comparison contract is missing",
                    "binary_hash": "oracle binary digest missing"}
    for check in authored["1"]["deliberate_mismatches"]:
        name = check["name"]
        directory = run / "skin-authored-1"
        if name in skin_headers:
            require(skin_headers[name] in (directory / f"{name}.stderr.txt").read_text(encoding="utf-8"),
                    f"Skin {name} failed for an unrelated reason")
            errors = [skin_headers[name]]
        else:
            expected = {"source_hash": "oracle source digest or byte length differs",
                        "source_length": "oracle source digest or byte length differs",
                        "tuple": "oracle bethesda_version differs",
                        "block_omission": "oracle skin block count differs",
                        "owner_omission": "oracle skin owner associations differ",
                        "owner_vertex_count": "oracle skin owner associations differ"}.get(name, "oracle skin fields differ at block")
            errors = rejection(document(directory / f"result-{name}.json"), expected)
        skin_diagnostics.append({"name": name, "diagnostics": errors})
    binding_diagnostics = []
    binding_headers = {"scope": "oracle decoded graph binding contract is missing",
                       "raw_contract": "oracle decoded graph binding contract is missing",
                       "graph_contract": "oracle decoded graph binding contract is missing",
                       "schema": "oracle provenance or raw-field comparison contract is missing",
                       "binary_hash": "oracle binary digest missing"}
    for check in authored["3"]["deliberate_mismatches"]:
        name = check["name"]
        directory = run / "skin-authored-3"
        if name in binding_headers:
            require(binding_headers[name] in (directory / f"{name}.stderr.txt").read_text(encoding="utf-8"),
                    f"Binding {name} failed for an unrelated reason")
            errors = [binding_headers[name]]
        else:
            errors = rejection(document(directory / f"rust-{name}.json"),
                               "oracle authored node fields or decoded graph binding facts differ")
        binding_diagnostics.append({"name": name, "diagnostics": errors})
    # Retain field-specific comparison failures from all schema-2 altered reports.
    partition_diagnostics = []
    header_cases = {"schema": "oracle provenance or raw-field comparison contract is missing",
                    "branch": "oracle partition branch contract is missing",
                    "raw_contract": "oracle partition branch contract is missing",
                    "binary_hash": "oracle binary digest missing"}
    for check in authored["2"]["deliberate_mismatches"]:
        name = check["name"]
        directory = run / "skin-authored-2"
        if name in header_cases:
            text = (directory / f"{name}.stderr.txt").read_text(encoding="utf-8")
            require(header_cases[name] in text, f"Partition {name} failed for an unrelated reason")
            errors = [header_cases[name]]
        else:
            expected = "oracle skin owner associations differ" if name == "owner_vertex_count" else (
                       "oracle skin fields differ at block" if name == "skin_root" else "oracle partition source fields differ")
            errors = rejection(document(directory / f"rust-{name}.json"), expected)
        partition_diagnostics.append({"name": name, "diagnostics": errors})
    # Owner source tests catch failures, but do not retain all malformed-source
    # stderr. Capture those four cases again and require their intended reason.
    malformed = []
    for name, native_reason, rust_reason in (
        ("flag", "unsupported noncanonical partition presence byte", "noncanonical partition presence byte"),
        ("width", "unsupported nonempty partition weight width", "nonempty partition arrays with width"),
        ("truncated", "partition array exceeds block/element budget", "NIF array exceeds block or element budget"),
        ("surplus", "partition source has surplus bytes", "unconsumed bytes in supported NIF block"),
    ):
        source = run / "skin-authored-2" / f"rejected-{name}.blob"
        native_path = run / f"partition-{name}-native.json"
        execute(f"partition-{name}-native", [skin, source, "--include-partitions"], expected=1, output=native_path)
        native_report = document(native_path)
        binary_identity(native_report, binaries[skin.relative_to(root).as_posix()])
        require(len(native_report["files"]) == 1 and native_reason in native_report["files"][0]["error"],
                f"Native partition {name} failed for an unrelated reason")
        rust_path = run / f"partition-{name}-rust.json"
        execute(f"partition-{name}-rust", [cli, "nif-skin", source, "--include-partitions", "--output", rust_path], expected=1)
        errors = rejection(document(rust_path), rust_reason)
        malformed.append({"name": name, "native_diagnostic": native_report["files"][0]["error"], "rust_diagnostics": errors})
    changed = document(run / "skin-authored-2/oracle.json")
    changed["oracle_binary_sha256"] = "0" * 64 if changed["oracle_binary_sha256"] != "0" * 64 else "1" * 64
    write_json(run / "skin-authored-2/altered-valid-binary-digest.json", changed)
    try:
        binary_identity(changed, binaries[skin.relative_to(root).as_posix()])
    except RuntimeError as error:
        require(str(error) == "Oracle report belongs to a different binary", "Binary guard failed for an unrelated reason")
        digest_diagnostic = str(error)
    else:
        raise RuntimeError("Changed valid-length binary digest was accepted")
    del changed

    archives = [install / "Data/Fallout - Meshes.bsa"] + sorted((install / "Data").glob("* - Main.bsa"))
    sample_env = environment.copy()
    sample_env["ASSET_SKIN_ARCHIVES"] = json.dumps([str(path) for path in archives])
    sample_env["ASSET_SKIN_EVIDENCE"] = str(run / "skin-retail")
    execute("skin-sample", [*powershell, root / "tools/cargo.ps1", "test", "--locked", "-p", "fallout-data",
            "--test", "nif_skin_samples", "--", "--ignored", "--nocapture"], env=sample_env)
    manifest = document(run / "skin-retail/source-manifest.json")
    require(manifest["samples"], "No original skin streams selected")
    sample_inputs = {}
    for row in manifest["samples"]:
        path = run / "skin-retail/inputs" / row["file"]
        require(path.parent == run / "skin-retail/inputs" and digest(path) == row["sha256"]
                and path.stat().st_size == row["decoded_bytes"], "Sample source identity differs")
        require(row["file"] not in sample_inputs, "Duplicate source sample")
        sample_inputs[row["file"]] = row["sha256"]
    skin_reports = {}
    for schema in (1, 2, 3):
        native_path, rust_path = run / f"skin-retail/native-{schema}.json", run / f"skin-retail/rust-{schema}.json"
        mode = ["--include-bindings"] if schema == 3 else (["--include-partitions"] if schema == 2 else [])
        execute(f"skin-native-{schema}", [sys.executable, root / "tools/run-nif-skin-oracle.py", "--binary", skin,
                "--input", run / "skin-retail/inputs", "--output", native_path, *mode])
        binary_identity(document(native_path), binaries[skin.relative_to(root).as_posix()])
        execute(f"skin-rust-{schema}", [cli, "nif-skin", run / "skin-retail/inputs", "--oracle-report", native_path,
                "--output", rust_path, *mode])
        report = document(rust_path)
        skin_report(report, manifest, binaries[skin.relative_to(root).as_posix()], schema)
        skin_reports[str(schema)] = report
    bindings = skin_reports["3"]
    require(bindings["binding_scope"] == "decoded-source-forest", "Skin source ancestry scope differs")
    binding_counts = {"nodes": 0, "instances": 0, "bone_references": 0, "owners": 0,
                      "scoped_membership_true": 0, "unsupported_scene_edges": 0,
                      "binding_diagnostics": bindings["binding_diagnostics"]}
    for row in bindings["files"]:
        graph = row["bindings"]
        require(graph["ancestry_scope"] == "decoded-source-forest", "File source ancestry scope differs")
        binding_counts["nodes"] += len(graph["nodes"])
        binding_counts["unsupported_scene_edges"] += len(graph["unsupported_scene_edges"])
        for instance in graph["instances"]:
            binding_counts["instances"] += 1
            binding_counts["bone_references"] += len(instance["bones"])
            binding_counts["owners"] += len(instance["owners"])
            binding_counts["scoped_membership_true"] += sum(bone["decoded_root_contains"] is True for bone in instance["bones"])

    execute("runtime-regression", [runner, "--checkpoint", "43", "--install", install,
            "--run-directory", regression, "--no-publish"])
    verified = document(regression / "reports/checkpoint-43-verification.json")
    require(verified["engine_revision"] == revision and verified["original_installation_matches_baseline"] is True,
            "Full runtime regression revision/baseline differs")
    require(document(regression / "reports/checkpoint-43-source-snapshot.json")["sha256"] == before["sha256"],
            "Full regression source snapshot differs")
    frame_path = regression / "event-frames-rust.json"
    require(document(frame_path)["sources"] == cold["sources"], "Actor and runtime source cohorts differ")
    operand_directory = run / "operands"
    operand_directory.mkdir()
    # The cached reader requires its root to exist before the first cold load.
    cache_directory = operand_directory / "index-cache"
    cache_directory.mkdir()
    cold_path, warm_path = operand_directory / "cold.json", operand_directory / "warm.json"
    for phase, path in (("cold", cold_path), ("warm", warm_path)):
        execute(f"event-operands-{phase}", [cli, "event-operands", "--install", install, "--load-order", order,
                "--index-cache", cache_directory, "--output", path], expected=1,
                diagnostic="Pending operands retain unresolved source or storage findings")
    operand_cold, operand_warm = document(cold_path), document(warm_path)
    operand_cold.pop("index_cache", None); operand_warm.pop("index_cache", None)
    require(operand_cold == operand_warm, "Cold/warm operand facts differ")
    del operand_cold, operand_warm
    quest = long_path(regression / FOREIGN / "quest-script-regression")
    binding_rust = quest / "operand-regression/operand-bindings-rust.json"
    bundle = quest / "operand-regression/operand-bindings.bin"
    native_operands = operand_directory / "native-tuples.json"
    execute("native-operand-tuples", [operand, bundle, install / "FalloutNV.exe", "--export-uses"], output=native_operands)
    loaded = quest / "loaded-script-regression/loaded-scripts-rust.json"
    saved = long_path(regression / FOREIGN / "native-save-regression/native-repository/golden-previous.frsv")
    audit_path = operand_directory / "audit.json"
    execute("operand-source-and-storage-audit", [sys.executable, root / "tools/integration_operand_audit.py",
            "--probe", cold_path, "--frames", frame_path, "--loaded-scripts", loaded, "--snapshot", saved,
            "--bindings-rust", binding_rust, "--bindings-native", native_operands,
            "--bindings-bundle", bundle, "--output", audit_path])
    audit = document(audit_path)
    require(len(audit["altered_evidence_diagnostics"]) == 10 and audit["accepted_scenarios"] == [],
            "Operand negative coverage or capability differs")

    require(git(root, "rev-parse", "HEAD").decode().strip() == revision and snapshot(root, revision) == before,
            "Candidate source/HEAD changed during proof")
    require(frozen_binaries(root) == binaries and frozen_research(root) == research, "Frozen binary/research inputs changed")
    require(all(digest(root / name) == expected for name, expected in inputs.items()), "Frozen profile/evidence/build input changed")
    require(all(digest(run / "skin-retail/inputs" / name) == expected for name, expected in sample_inputs.items()),
            "Sampled source changed during proof")
    write_json(run / "frozen-finish.json", {"source": before, "binaries": binaries, "inputs": inputs,
               "research": research, "build_receipt": str(build_path.relative_to(root))})
    write_json(run / "integration.json", {"schema_version": 1, "checkpoint": 45, "engine_revision": revision,
               "source_snapshot_sha256": before["sha256"], "binaries": binaries, "inputs": inputs,
               "research": research, "commands": commands, "source_and_binaries_unchanged": True,
               "actor_findings": retained, "actor_negative_diagnostics": actor_negatives,
               "actor_counts": {name: cold[name]["counts"] for name in ("actor_associations", "actor_classes", "actor_factions", "actor_placements")},
               "actor_authored": actor_authored, "actor_body_allocation_order_checks_passed": 6,
               "skin_authored": authored, "skin_partition_negative_diagnostics": partition_diagnostics,
               "skin_negative_diagnostics": skin_diagnostics,
               "skin_binding_negative_diagnostics": binding_diagnostics, "skin_source_binding_counts": binding_counts,
               "skin_source_rejection_diagnostics": malformed, "changed_valid_binary_digest_diagnostic": digest_diagnostic,
               "skin_sample_manifest_sha256": digest(run / "skin-retail/source-manifest.json"),
               "skin_sampled_files": len(sample_inputs), "skin_sampled_counts": {schema: {
                   "block_counts": report["block_counts"], "owners": report["owners"],
                   "unresolved_dependencies": report["unresolved_dependencies"]} for schema, report in skin_reports.items()},
               "operand_audit": audit, "runtime_regression": {"checkpoint_scope": 43,
                   "directory": str(regression.relative_to(root)), "verification_sha256": digest(regression / "reports/checkpoint-43-verification.json"),
                   "tests_passed": verified["tests_passed"], "python_publication_tests_passed": verified["python_publication_tests_passed"]},
               "original_installation_matches_baseline": True, "installation_files_checked": verified["installation_files_checked"],
               "installation_bytes_checked": verified["installation_bytes_checked"], "finished_unix_seconds_utc": int(time.time()),
               "bytecode_executed": False, "runtime_skinning_accepted": False, "retail_parity_accepted": False,
               "accepted_scenarios": []})
    print(f"Verified checkpoint-45 candidate {revision}; raw evidence remains in {run}")


if __name__ == "__main__":
    main()
