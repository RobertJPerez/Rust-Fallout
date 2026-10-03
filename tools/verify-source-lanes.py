"""Verify integrated actor and skin source slices against frozen source and binaries.

Raw reports and sampled assets stay local. This runner records source facts and
engineering regressions; it does not accept actor behavior or evaluated skinning.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def document(path):
    return json.loads(path.read_bytes())


def write_json(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, indent=2)
        output.write("\n")


def git(root, *arguments):
    return subprocess.check_output(["git", "-C", str(root), *arguments])


def snapshot(root, revision):
    names = git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z")
    files = []
    for name in sorted(set(names.decode().split("\0")) - {""}):
        path = Path(name)
        if path.suffix not in {".rs", ".wgsl", ".py", ".ps1", ".toml", ".cpp", ".hpp"} and path.name not in {"Cargo.lock", "CMakeLists.txt", "sources.lock.json"}:
            continue
        current = digest(root / name)
        committed = hashlib.sha256(git(root, "show", f"{revision}:{name}")).hexdigest()
        require(current == committed, f"Source differs from commit: {name}")
        files.append({"path": name, "sha256": current})
    encoded = json.dumps(files, separators=(",", ":"), ensure_ascii=False).encode()
    return {"schema_version": 1, "revision": revision, "files": files,
            "working_tree_dirty": bool(git(root, "status", "--porcelain")),
            "sha256": hashlib.sha256(encoded).hexdigest(),
            "digest_recipe": "SHA256 of compact UTF-8 JSON files array; object keys path then sha256"}


def binary_identity(report, expected):
    # Hex syntax alone cannot bind an oracle report to the reader actually run.
    require(report.get("oracle_binary_sha256") == expected, "Oracle report belongs to a different binary")


def new_local_path(root, path):
    path = (root / path).resolve()
    require(path.parent == (root / "local").resolve(), "Use a new directory immediately under repository local")
    require(not path.exists(), f"Evidence directory already exists: {path}")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path("."))
    parser.add_argument("--install", type=Path, required=True)
    parser.add_argument("--run-directory", type=Path, required=True)
    parser.add_argument("--regression-directory", type=Path, required=True)
    args = parser.parse_args()
    root = args.repository.resolve()
    install = args.install.resolve()
    run = new_local_path(root, args.run_directory)
    regression = new_local_path(root, args.regression_directory)
    require(run != regression, "Lane and runtime evidence directories must differ")
    require(not git(root, "status", "--porcelain"), "Commit integrated source before verification")
    revision = git(root, "rev-parse", "HEAD").decode().strip()
    before = snapshot(root, revision)
    order_path = root / "profiles/nv-inspection-order.json"
    order_digest = digest(order_path)
    cli = root / "target/release/fallout.exe"
    runner = root / "target/release/fallout-evidence.exe"
    actor = root / "local/actor-oracle-build/Release/actor-oracle.exe"
    skin = root / "local/nif-skin-oracle-build/Release/nif-skin-oracle.exe"
    binaries = {str(path.relative_to(root)): digest(path) for path in (cli, runner, actor, skin)}
    run.mkdir()
    write_json(run / "source-snapshot.json", before)
    commands = []

    def execute(name, arguments, expected=0, environment=None):
        arguments = [str(value) for value in arguments]
        print(f"Running {name}", flush=True)
        with (run / f"{name}.log").open("xb") as log:
            result = subprocess.run(arguments, cwd=root, env=environment, stdout=log, stderr=subprocess.STDOUT)
        commands.append({"name": name, "arguments": arguments, "exit_code": result.returncode,
                         "log_sha256": digest(run / f"{name}.log")})
        require(result.returncode == expected, f"{name} failed; see {run / (name + '.log')}")

    powershell = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
    execute("source-lane-guard-tests", [sys.executable, root / "tools/test_source_lane_guards.py"])
    execute("actors", [*powershell, root / "tools/actor-oracle/compare.ps1", "-Fallout", cli,
            "-Oracle", actor, "-Install", install, "-LoadOrder", root / "profiles/nv-inspection-order.json",
            "-RunDirectory", run / "actors"])
    actor_receipt = document(run / "actors/worker-comparison.json")
    require(actor_receipt["engine_binary_sha256"] == binaries[str(cli.relative_to(root))], "Actor CLI binary differs")
    require(actor_receipt["oracle_binary_sha256"] == binaries[str(actor.relative_to(root))], "Actor oracle binary differs")
    actor_phases = []
    original_actors = document(run / "actors/cold.json")
    for name in ("cold", "warm", "reordered"):
        report = document(run / f"actors/{name}.json")
        require(report["independent_comparison"]["equal"] is True, "Actor comparison is incomplete")
        require(report["definitions"] == original_actors["definitions"], "Actor reorder changed definitions")
        require(report["winning_content_sha256"] == original_actors["winning_content_sha256"], "Actor winners differ")
        require(sorted(report["sources"], key=lambda row: row["source_name"].lower()) ==
                sorted(original_actors["sources"], key=lambda row: row["source_name"].lower()), "Actor full source cohort differs")
        require(report["counts"]["source_findings"] == 0, "Actor findings must remain explicit")
        require(report["actors_initialized"] is False and report["retail_parity_accepted"] is False, "Actor scope differs")
        actor_phases.append({"name": name, "report_sha256": digest(run / f"actors/{name}.json"),
                             "index_cache": report["index_cache"]})
    altered = document(run / "actors/oracle.json")
    scalar = next(field for row in altered["definitions"] for field in row["fields"] if field.get("value", {}).get("kind") == "configuration")
    scalar["value"]["fatigue"] ^= 1
    write_json(run / "actors/altered-fatigue.json", altered)
    execute("actors-altered-fatigue", [cli, "actor-sources", "--install", install,
            "--load-order", root / "profiles/nv-inspection-order.json", "--compare-oracle",
            run / "actors/altered-fatigue.json", "--output", run / "actors/altered-result.json"], expected=1)
    require(b"independent actor source comparison differs in definitions" in
            (run / "actors-altered-fatigue.log").read_bytes(), "Actor negative failed for an unrelated reason")

    execute("skin-authored", [sys.executable, root / "tools/nif-skin-oracle/check_comparison.py",
            "--binary", cli, "--oracle", skin, "--output-dir", run / "skin-authored"])
    authored = document(run / "skin-authored/summary.json")
    binary_identity(document(run / "skin-authored/oracle.json"), binaries[str(skin.relative_to(root))])
    require(authored["cli_sha256"] == binaries[str(cli.relative_to(root))], "Skin CLI binary differs")
    require(authored["oracle_sha256"] == binaries[str(skin.relative_to(root))], "Skin oracle binary differs")
    changed = document(run / "skin-authored/oracle.json")
    changed["oracle_binary_sha256"] = "0" * 64 if changed["oracle_binary_sha256"] != "0" * 64 else "1" * 64
    write_json(run / "skin-authored/altered-valid-binary-digest.json", changed)
    try:
        binary_identity(changed, binaries[str(skin.relative_to(root))])
    except RuntimeError:
        changed_digest_rejected = True
    else:
        raise RuntimeError("Valid-length changed oracle digest was accepted")

    archives = [install / "Data/Fallout - Meshes.bsa"] + sorted((install / "Data").glob("* - Main.bsa"))
    sample_environment = os.environ.copy()
    sample_environment["ASSET_SKIN_ARCHIVES"] = json.dumps([str(path) for path in archives])
    sample_environment["ASSET_SKIN_EVIDENCE"] = str(run / "skin-retail")
    execute("skin-sample", [*powershell, root / "tools/cargo.ps1", "test", "--locked", "-p", "fallout-data",
            "--test", "nif_skin_samples", "--", "--ignored", "--nocapture"], environment=sample_environment)
    manifest = document(run / "skin-retail/source-manifest.json")
    require(bool(manifest["samples"]), "No original skins were selected")
    for sample in manifest["samples"]:
        path = run / "skin-retail/inputs" / sample["file"]
        require(path.parent == run / "skin-retail/inputs", "Invalid sample filename")
        require(digest(path) == sample["sha256"] and path.stat().st_size == sample["decoded_bytes"], "Sample input differs")
    execute("skin-native", [sys.executable, root / "tools/run-nif-skin-oracle.py", "--binary", skin,
            "--input", run / "skin-retail/inputs", "--output", run / "skin-retail/oracle.json"])
    native = document(run / "skin-retail/oracle.json")
    binary_identity(native, binaries[str(skin.relative_to(root))])
    execute("skin-rust", [cli, "nif-skin", run / "skin-retail/inputs", "--oracle-report",
            run / "skin-retail/oracle.json", "--output", run / "skin-retail/rust.json"])
    retail = document(run / "skin-retail/rust.json")
    require(retail["failures"] == 0 and retail["runtime_ready"] is False, "Skin scope or comparisons differ")
    binary_identity(retail, binaries[str(skin.relative_to(root))])
    require(len(retail["files"]) == len(manifest["samples"]), "Sample coverage differs")
    require(all(row["comparison"] == "all_equal" and row["error"] is None for row in retail["files"]), "Partial skin comparison")

    # Run the existing runtime verification surface at this new source revision.
    # Its checkpoint selector names the regression scope, not a new publication.
    execute("runtime-regression", [runner, "--checkpoint", "43", "--install", install,
            "--run-directory", regression, "--no-publish"])
    verified = document(regression / "reports/checkpoint-43-verification.json")
    regression_source = document(regression / "reports/checkpoint-43-source-snapshot.json")
    frames = document(regression / "reports/checkpoint-43-event-frames.json")
    require(frames["sources"] == original_actors["sources"], "Actor and runtime source cohorts differ")
    require(regression_source["sha256"] == before["sha256"] and verified["engine_revision"] == revision, "Regression source differs")
    require(verified["original_installation_matches_baseline"] is True, "Installation baseline differs")
    require(git(root, "rev-parse", "HEAD").decode().strip() == revision, "HEAD changed during verification")
    require(snapshot(root, revision) == before, "Source changed during verification")
    require(digest(order_path) == order_digest, "Inspection profile changed during verification")
    require(all(digest(root / name) == value for name, value in binaries.items()), "A verification binary changed")
    actor_result = {"counts": original_actors["counts"], "sources": original_actors["sources"],
                    "winning_content_sha256": original_actors["winning_content_sha256"], "phases": actor_phases,
                    "complete_physical_fields_equal": True, "altered_fatigue_rejected": True,
                    "source_findings": [], "actors_initialized": False}
    skin_result = {"authored": authored, "sampled_files": len(manifest["samples"]),
                   "source_manifest_sha256": digest(run / "skin-retail/source-manifest.json"),
                   "rust_report_sha256": digest(run / "skin-retail/rust.json"),
                   "oracle_report_sha256": digest(run / "skin-retail/oracle.json"),
                   "decoded_scan_bytes": manifest["decoded_scan_bytes"], "selection_findings": manifest["findings"],
                   "block_counts": retail["block_counts"], "owners": retail["owners"],
                   "unresolved_dependencies": retail["unresolved_dependencies"],
                   "actual_oracle_binary_bound": True, "changed_valid_binary_digest_rejected_by_runner": changed_digest_rejected,
                   "runtime_ready": False}
    receipt = {"schema_version": 1, "checkpoint": 44, "engine_revision": revision,
               "source_snapshot_sha256": before["sha256"], "binaries": binaries, "commands": commands,
               "inspection_order_sha256": order_digest, "actor_and_runtime_source_cohorts_equal": True,
               "actors": actor_result, "skin": skin_result,
               "runtime_regression": {"checkpoint_scope": 43, "directory": str(regression.relative_to(root)),
                                      "verification_sha256": digest(regression / "reports/checkpoint-43-verification.json"),
                                      "tests_passed": verified["tests_passed"], "python_publication_tests_passed": verified["python_publication_tests_passed"]},
               "original_installation_matches_baseline": True,
               "installation_files_checked": verified["installation_files_checked"],
               "installation_bytes_checked": verified["installation_bytes_checked"],
               "source_and_binaries_unchanged": True, "finished_unix_seconds_utc": int(time.time()),
               "source_lane_guard_tests_passed": 3,
               "retail_parity_accepted": False, "accepted_scenarios": []}
    write_json(run / "source-lanes.json", receipt)
    print(f"Verified integrated source lanes at {revision}; raw evidence remains in {run}")


if __name__ == "__main__":
    main()
