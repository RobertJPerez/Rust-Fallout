"""Shared checks for the additive checkpoint-45 integration proof."""
import importlib.util
from pathlib import Path


spec = importlib.util.spec_from_file_location("source_lanes_44", Path(__file__).with_name("verify-source-lanes.py"))
lanes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lanes)
require = lanes.require
digest = lanes.digest
document = lanes.document
write_json = lanes.write_json
git = lanes.git
snapshot = lanes.snapshot
binary_identity = lanes.binary_identity
new_local_path = lanes.new_local_path


FOREIGN = Path("source-item-regression/native-migration-regression/item-state-regression/leveled-source-regression/base-inventory-regression/foreign-runtime-regression")
HISTORICAL = (
    ("local/census-with-scripts.json", "reports/checkpoint-15-compiled-scripts.json", "prior_full_census_sha256"),
    ("local/bindings-17-verified/bindings-rust.json", "reports/checkpoint-17-script-bindings.json", "rust_report_sha256"),
    ("local/bindings-17-verified/bindings.bin", "reports/checkpoint-17-script-bindings.json", "comparison_bundle_sha256"),
    ("local/condition-fields-21-verified/conditions-rust.json", "reports/checkpoint-21-condition-fields.json", "rust_report_sha256"),
    ("local/loaded-scripts-24-verified/loaded-scripts-rust.json", "reports/checkpoint-24-loaded-scripts.json", "rust_report_sha256"),
)


def long_path(path):
    # Existing full regression artifacts have nested names exceeding MAX_PATH.
    path = path.resolve()
    if str(path).startswith("\\\\") or not path.drive:
        return path
    return Path("\\\\?\\" + str(path))


def historical_inputs(root):
    values = {}
    for name, receipt, field in HISTORICAL:
        actual = digest(root / name)
        require(actual == document(root / receipt)[field], f"Historical evidence differs: {name}")
        values[name] = actual
        values[receipt] = digest(root / receipt)
    return values


def frozen_inputs(root):
    values = historical_inputs(root)
    values["local/baseline.json"] = digest(root / "local/baseline.json")
    values["local/integration-seed.json"] = digest(root / "local/integration-seed.json")
    for path in sorted((root / "profiles").glob("*.json")):
        values[path.relative_to(root).as_posix()] = digest(path)
    manifest = root / "local/integration-actor-fixtures/manifest.json"
    values[manifest.relative_to(root).as_posix()] = digest(manifest)
    for row in document(manifest)["files"]:
        path = (root / row["path"]).resolve()
        require(path.is_relative_to(manifest.parent.resolve()), "Authored fixture escapes private input directory")
        require(digest(path) == row["sha256"] and path.stat().st_size == row["bytes"],
                f"Authored actor input differs: {row['path']}")
        values[row["path"]] = row["sha256"]
    return dict(sorted(values.items()))


def frozen_binaries(root):
    seed = document(root / "local/integration-seed.json")
    names = {row["path"] for row in seed["files"] if row["path"].endswith(".exe")}
    names.update(("target/release/fallout.exe", "target/release/fallout-evidence.exe"))
    names.add("local/actor-oracle-build/Release/actor-body-tests.exe")
    values = {name: digest(root / name) for name in sorted(names)}
    changed = {f"local/{name}-oracle-build/Release/{name}-oracle.exe" for name in ("actor", "operand", "nif-skin")}
    for row in seed["files"]:
        if row["path"].endswith(".exe") and row["path"] not in changed:
            require(values[row["path"]] == row["sha256"], f"Untouched seeded binary differs: {row['path']}")
    return values


def frozen_research(root):
    directories = {"https://github.com/TES5Edit/TES5Edit": "xedit",
                   "https://github.com/niftools/nifxml": "nifxml",
                   "https://github.com/ousnius/nifly": "nifly",
                   "https://github.com/xNVSE/NVSE": "NVSE"}
    result = {}
    for reference in document(root / "sources.lock.json")["references"]:
        if reference["url"] not in directories:
            continue
        name = directories[reference["url"]]
        repository = root / ".research" / name
        revision = git(repository, "rev-parse", "HEAD").decode().strip()
        require(revision == reference["commit"], f"Pinned upstream revision differs: {name}")
        require(not git(repository, "status", "--porcelain", "--untracked-files=no"), f"Pinned upstream source changed: {name}")
        files = {}
        for scope in reference.get("source_read_scopes", []):
            if scope.get("checkpoint") != 45:
                continue
            actual = digest(repository / scope["path"])
            require(actual == scope["whole_file_sha256"], f"Reviewed upstream source differs: {name}/{scope['path']}")
            files[scope["path"]] = actual
        require(files, f"Candidate source scopes missing: {name}")
        result[name] = {"revision": revision, "files": files}
    require(set(result) == set(directories.values()), "Candidate research cohort differs")
    return result


def actor_findings(report):
    require(report["counts"]["source_findings"] == 0, "Unexpected actor scalar finding")
    for name in ("actor_classes", "actor_factions", "actor_placements"):
        require(report[name]["counts"]["source_findings"] == 0, f"Unexpected {name} finding")
    associations = report["actor_associations"]
    findings = []
    for row in associations["definitions"]:
        for finding in row["findings"]:
            findings.append({"key": row["key"], **finding})
    require(len(findings) == associations["counts"]["source_findings"] == 1,
            "Association source finding coverage differs")
    finding = findings[0]
    require(finding == {
        "key": {"origin_plugin": "deadmoney.esm", "local_id": 0xAE30, "profile": "nv-original"},
        "field_decoded_offset": 183, "code": "association_target_deleted",
    }, "Expected deleted voice finding differs")
    matches = []
    for row in associations["definitions"]:
        for association in row["associations"]:
            if association["binding"]["status"] == "deleted":
                scalar = next(value for value in report["definitions"] if value["key"] == row["key"])
                field = scalar["fields"][association["field_index"]]
                matches.append({"key": row["key"], "field_decoded_offset": field["decoded_offset"],
                                "role": association["role"], "binding": association["binding"],
                                "schema_kind_allowed": association["schema_kind_allowed"]})
    require(len(matches) == 1, "Deleted voice binding was omitted or duplicated")
    value = matches[0]
    require(value["key"] == finding["key"] and value["field_decoded_offset"] == 183
            and value["role"] == "voice" and value["schema_kind_allowed"] is True,
            "Deleted voice occurrence differs")
    binding = value["binding"]
    require(binding["key"] == {"origin_plugin": "falloutnv.esm", "local_id": 0x29FB1, "profile": "nv-original"}
            and binding["raw_form"] == 171953
            and binding["target"]["source_plugin"].lower() == "oldworldblues.esm"
            and binding["target"]["record_file_offset"] == 16149040
            and binding["target"]["kind"] == list(b"VTYP")
            and binding["target"]["record_flags"] == 32,
            "Deleted voice winning provenance differs")
    return {"findings": findings, "deleted_binding": value}


def skin_report(report, manifest, expected_binary, schema):
    binary_identity(report, expected_binary)
    require(report["schema_version"] == schema and report["failures"] == 0
            and report["runtime_ready"] is False, "Skin comparison or scope differs")
    names = [Path(row["input"]).name for row in report["files"]]
    wanted = [row["file"] for row in manifest["samples"]]
    require(len(set(names)) == len(names) and sorted(names) == sorted(wanted), "Skin sample occurrence coverage differs")
    require(all(row["comparison"] == "all_equal" and row["error"] is None for row in report["files"]),
            "Partial skin comparison")


def rejection(report, expected):
    errors = [row["error"] for row in report["files"] if row.get("error")]
    require(report["failures"] > 0 and errors, "Altered report lacks comparison diagnostic")
    require(all(expected in error for error in errors), f"Altered report failed for an unrelated reason: {errors}")
    return errors
