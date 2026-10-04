"""Fresh source-local skin proof; invoke once inside team-build's heavy admission.

This builds the committed producer and the pinned, separate native reader. Native
source fields must compare exactly before the existing rational palette checker
runs. Stored locals, raw weights and unapplied controllers remain engineering
inputs; this driver does not observe original execution or animation playback.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tarfile
import time


NIFLY_REVISION = "cca0a770094bb962fb28ea1fec5ea903e68fda8e"
CONTRACT = "engineering-source-local-skin-v1"
MAX_SOURCE_BYTES = 512 * 1024 * 1024
MAX_FILES = 20_000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def identity(path):
    with Path(path).open("rb") as source:
        size = os.fstat(source.fileno()).st_size
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    return {"bytes": size, "sha256": digest}


def retain_file(records, path, expected=None):
    """Keep the first complete identity; later observations can only verify it."""
    path = Path(path).resolve()
    current = identity(path)
    if expected is not None:
        require(current == expected, f"Validated artifact changed: {path}")
    key = str(path)
    row = {"path": key, **current}
    if key in records:
        require(row == records[key], f"Retained artifact changed: {path}")
        return records[key]
    records[key] = row
    return row


def write_json(path, value):
    with Path(path).open("x", encoding="utf-8", newline="\n") as output:
        json.dump(value, output, indent=2, allow_nan=False)
        output.write("\n")


def document(path, limit=128 * 1024 * 1024):
    require(Path(path).stat().st_size <= limit, "JSON exceeds byte admission")
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def git(root, *arguments):
    return subprocess.check_output(["git", "-C", str(root), *arguments])


def clean_head(root, revision):
    require(git(root, "rev-parse", "HEAD").decode().strip() == revision,
            "Repository HEAD differs from explicit revision")
    require(not git(root, "status", "--porcelain", "--untracked-files=all"),
            "Repository must be clean and committed")


def safe_relative(name):
    path = PurePosixPath(name)
    require(bool(name) and not path.is_absolute() and ".." not in path.parts
            and "\\" not in name and ":" not in name,
            "Source package path escapes its root")
    require(all(part not in {"", ".", ".git"} for part in path.parts),
            "Invalid source package path")
    return path.as_posix()


def tracked_names(root, revision):
    rows = git(root, "ls-tree", "-rz", "--full-tree", revision).split(b"\0")
    names = []
    for row in rows:
        if not row:
            continue
        metadata, name = row.split(b"\t", 1)
        mode, kind, _ = metadata.split(b" ")
        require(mode in {b"100644", b"100755"} and kind == b"blob",
                "Proof sources cannot contain symlinks or unfrozen submodules")
        names.append(safe_relative(name.decode("utf-8")))
    require(0 < len(names) <= MAX_FILES, "Source file count exceeds admission")
    require(len({name.casefold() for name in names}) == len(names),
            "Source names collide on Windows")
    return sorted(names)


def source_snapshot(root, revision):
    clean_head(root, revision)
    rows = []
    total = 0
    for name in tracked_names(root, revision):
        path = root / name
        require(not path.is_symlink() and path.resolve().is_relative_to(root.resolve()),
                "Tracked source leaves repository")
        value = identity(path)
        total += value["bytes"]
        require(total <= MAX_SOURCE_BYTES, "Source byte count exceeds admission")
        rows.append({"path": name, **value})
    encoded = json.dumps(rows, separators=(",", ":"), ensure_ascii=False).encode()
    return {"revision": revision, "files": rows,
            "sha256": hashlib.sha256(encoded).hexdigest(),
            "digest_recipe": "SHA256 of compact UTF-8 files array; keys path,bytes,sha256"}


def archived_snapshot(archive, revision):
    # Hash the committed blobs independently of checkout filters/line endings.
    rows, total = [], 0
    with tarfile.open(archive, mode="r:") as stream:
        for member in stream:
            name = safe_relative(member.name.rstrip("/"))
            if member.isdir():
                continue
            require(member.isfile(), "Source archive contains a link or special entry")
            total += member.size
            require(total <= MAX_SOURCE_BYTES and len(rows) < MAX_FILES,
                    "Source archive exceeds admission")
            source = stream.extractfile(member)
            require(source is not None, "Source archive member unavailable")
            with source:
                digest = hashlib.file_digest(source, "sha256").hexdigest()
            rows.append({"path": name, "bytes": member.size, "sha256": digest})
    rows.sort(key=lambda row: row["path"])
    require(rows and len({row["path"].casefold() for row in rows}) == len(rows),
            "Duplicate/empty source archive")
    encoded = json.dumps(rows, separators=(",", ":"), ensure_ascii=False).encode()
    return {"revision": revision, "files": rows,
            "sha256": hashlib.sha256(encoded).hexdigest(),
            "digest_recipe": "SHA256 of compact UTF-8 files array; keys path,bytes,sha256"}


def checked_identity(row):
    require(type(row.get("bytes")) is int and row["bytes"] >= 0,
            "Package needs an exact byte count")
    digest = row.get("sha256")
    require(isinstance(digest, str) and len(digest) == 64
            and all(char in "0123456789abcdef" for char in digest),
            "Package needs a lowercase SHA256")
    return {"bytes": row["bytes"], "sha256": digest}


def checked_package(path, expected):
    require(expected["bytes"] <= 8 * 1024 * 1024 and Path(path).stat().st_size <= 8 * 1024 * 1024,
            "Input package exceeds byte admission")
    require(identity(path) == expected, "Input package digest/length differs")
    package = document(path, 8 * 1024 * 1024)
    require(package.get("schema_version") == 1, "Unknown skin proof input package")
    selected = package["input"]
    input_path = Path(selected["path"]).resolve(strict=True)
    require(0 < selected["bytes"] <= 64 * 1024 * 1024,
            "Selected source input exceeds admission")
    require(input_path.stat().st_size <= 64 * 1024 * 1024, "Selected source input exceeds admission")
    require(identity(input_path) == checked_identity(selected),
            "Selected source digest/length differs")
    geometry = package["geometry"]
    tolerance = package["absolute_weight_tolerance"]
    require(type(geometry) is int and 0 <= geometry < 2**32 - 1,
            "Select a concrete owned geometry distinct from the refusal sentinel")
    require(type(tolerance) in {int, float} and math.isfinite(tolerance) and tolerance > 0,
            "A finite positive weight tolerance is required")
    native = package["native_source"]
    require(native["revision"] == NIFLY_REVISION, "Native source revision differs from pin")
    native_root = Path(native["path"]).resolve(strict=True)
    actual = source_snapshot(native_root, NIFLY_REVISION)
    expected_rows = native["files"]
    require(isinstance(expected_rows, list) and 0 < len(expected_rows) <= MAX_FILES,
            "Native source package must enumerate its actual tracked files")
    rows = [{"path": safe_relative(row["path"]), **checked_identity(row)} for row in expected_rows]
    rows.sort(key=lambda row: row["path"])
    require(rows == actual["files"], "Native source package differs or omits tracked files")
    return package, input_path, native_root, actual


def copy_verified(source, destination, expected):
    require(identity(source) == expected, "Copy source differs before freeze")
    with Path(source).open("rb") as input_stream, Path(destination).open("xb") as output:
        shutil.copyfileobj(input_stream, output)
    require(identity(source) == expected and identity(destination) == expected,
            "Source changed while freezing")


class Commands:
    def __init__(self, output, environment):
        self.output, self.environment, self.rows = output, environment, []
        self.evidence = {}

    def execute(self, name, arguments, cwd, stdout=None):
        arguments = [str(value) for value in arguments]
        output = stdout or self.output / (name + ".stdout.txt")
        error = self.output / (name + ".stderr.txt")
        row = {"name": name, "arguments": arguments, "cwd": str(cwd),
               "started_unix": time.time()}
        self.rows.append(row)
        started = self.output / (name + ".started.json")
        write_json(started, row)
        retain_file(self.evidence, started)
        try:
            with output.open("xb") as out, error.open("xb") as err:
                process = subprocess.run(arguments, cwd=cwd, env=self.environment,
                                         stdout=out, stderr=err, check=False)
            row["exit_code"] = process.returncode
            require(process.returncode == 0, f"{name} failed with exit {process.returncode}")
        except BaseException as failure:
            row["error"] = f"{type(failure).__name__}: {failure}"
            raise
        finally:
            row["finished_unix"] = time.time()
            row["outputs"] = [retain_file(self.evidence, path)
                              for path in (output, error) if path.is_file()]
            completed = self.output / (name + ".command.json")
            write_json(completed, row)
            retain_file(self.evidence, completed)


def freeze_repository(commands, original, revision, destination, archives, name):
    archive = archives / (name + ".tar")
    commands.execute(name + "-archive", ["git", "-C", original, "archive", "--format=tar", revision],
                     original, stdout=archive)
    expected = archived_snapshot(archive, revision)
    require(source_snapshot(original, revision) == expected,
            f"{name} checkout bytes differ from committed blobs")
    commands.execute(name + "-clone", ["git", "-c", "core.autocrlf=false", "clone", "--local",
                     "--no-hardlinks", "--no-checkout", original, destination], original)
    commands.execute(name + "-checkout", ["git", "-C", destination, "-c", "core.autocrlf=false",
                     "checkout", "--detach", revision], original)
    require(source_snapshot(destination, revision) == expected,
            f"{name} private checkout differs from committed archive")
    return expected


def validate_native(path, binary, selected):
    report = document(path)
    require(report.get("schema_version") == 3
            and report.get("nifly_revision") == NIFLY_REVISION
            and report.get("float_encoding") == "ieee754-binary32-bits"
            and report.get("binding_scope") == "decoded-source-forest"
            and report.get("partition_branch") == "nv-canonical-flags-four-wide-or-empty"
            and report.get("prepare_data_called") is False
            and all(report.get(field) is True for field in (
                "raw_presence_and_vertex_counts_checked", "raw_partition_fields_checked",
                "raw_node_fields_checked", "graph_membership_checked")),
            "Fresh native source contract is incomplete")
    require(report.get("oracle_binary_sha256") == identity(binary)["sha256"],
            "Fresh native report belongs to a different binary")
    rows = report.get("files")
    require(isinstance(rows, list) and len(rows) == 1, "Native selected-source coverage differs")
    row = rows[0]
    require(row.get("file") == selected.name and not row.get("error")
            and {"bytes": row.get("decoded_bytes"), "sha256": row.get("sha256")} == identity(selected),
            "Native selected source identity/length differs")
    return row


def validate_comparison(path, native, binary, selected):
    report = document(path)
    require(report.get("schema_version") == 3 and report.get("failures") == 0
            and report.get("runtime_ready") is False
            and report.get("binding_scope") == "decoded-source-forest",
            "Exact source comparison failed or claims runtime readiness")
    require(report.get("oracle_report_sha256") == identity(native)["sha256"]
            and report.get("oracle_binary_sha256") == identity(binary)["sha256"],
            "Source comparison is not bound to this fresh native report/binary")
    rows = report.get("files")
    require(isinstance(rows, list) and len(rows) == 1, "Rust selected-source coverage differs")
    row = rows[0]
    require(Path(row["input"]).resolve() == selected.resolve()
            and {"bytes": row.get("decoded_bytes"), "sha256": row.get("sha256")} == identity(selected)
            and row.get("comparison") == "all_equal" and row.get("error") is None,
            "Selected source was not compared completely")


def validate_pose(directory, binary, selected, native, package, native_row, evidence):
    summary_identity = identity(directory / "summary.json")
    summary = document(directory / "summary.json")
    require(summary.get("contract") == CONTRACT
            and summary.get("binary_sha256") == identity(binary)["sha256"]
            and summary.get("source_sha256") == identity(selected)["sha256"]
            and summary.get("native_report_sha256") == identity(native)["sha256"]
            and summary.get("retail_behavior_verified") is False
            and summary.get("intended_refusals") == 2
            and summary.get("altered_palette_rejections") == 1,
            "Rational pose checks lack exact identities/refusals/altered coefficient")
    geometry = package["geometry"]
    owner = next(row for row in native_row["owners"] if row["geometry"] == geometry)
    instance = next(row["data"]["instance"] for row in native_row["skins"]
                    if row["block"] == owner["instance"])
    numeric = summary["numeric"]
    require(numeric["vertices"] == owner["vertex_count"]
            and numeric["palette_entries"] == len(instance["bones"])
            and numeric["coefficients"] == 12 * len(instance["bones"])
            and numeric["coefficients"] > 0
            and math.isfinite(numeric["maximum_absolute_palette_error"])
            and numeric["numerical_bound"] == "gamma_(7*m) * absolute matrix product + 2^-1074",
            "Pose numeric coverage differs from independently decoded source")
    rows = summary.get("invocations")
    require(isinstance(rows, list) and len(rows) == 3, "Pose invocation coverage differs")
    for row, name, selected_geometry, tolerance, code in zip(rows,
            ("positive", "strict-zero-tolerance", "unowned-geometry"),
            (geometry, geometry, 2**32 - 1),
            (package["absolute_weight_tolerance"], 0.0, package["absolute_weight_tolerance"]),
            (0, 1, 1)):
        receipt = directory / (name + ".json")
        arguments = row["command"]
        expected = [str(binary.resolve()), "nif-skin", str(selected.resolve()),
                    "--pose-geometry", str(selected_geometry), "--pose-weight-tolerance", str(float(tolerance)),
                    "--output", str(receipt.resolve())]
        require(arguments == expected and row["exit_code"] == code
                and row["receipt_sha256"] == identity(receipt)["sha256"],
                "Pose invocation receipt/command identity differs")
        retain_file(evidence, receipt, {"bytes": receipt.stat().st_size, "sha256": row["receipt_sha256"]})
        for suffix in (".stdout.txt", ".stderr.txt"):
            retain_file(evidence, directory / (name + suffix))
    retain_file(evidence, directory / "summary.json", summary_identity)
    return summary


def run(args, command_factory=Commands):
    root = args.repository.resolve(strict=True)
    output = args.output_directory.resolve()
    require(output.parent == (root / "local").resolve() and not output.exists(),
            "Use a fresh directory immediately under the committed repository's local")
    output.mkdir()
    receipt = {"schema_version": 1, "contract": CONTRACT, "state": "failed",
               "repository": str(root), "revision": args.revision,
               "started_unix": time.time(), "engineering_proof_passed": False,
               "retail_behavior_verified": False, "playback_verified": False,
               "admission": "Caller must run this entire driver inside tools/team-build.py --slot heavy"}
    write_json(output / "started.json", {**receipt, "state": "started"})
    environment = os.environ.copy()
    commands = command_factory(output, environment)
    retain_file(commands.evidence, output / "started.json")
    originals, frozen, binaries, inputs = {}, {}, {}, {}
    failure = None

    def retain_binary(name, path):
        before = retain_file(commands.evidence, path)
        binaries.setdefault(name, path)
        receipt.setdefault("binaries_before", {}).setdefault(name, before)

    try:
        require(len(args.revision) == 40 and all(c in "0123456789abcdef" for c in args.revision),
                "Use an exact committed repository revision")
        clean_head(root, args.revision)
        require(git(root, "check-ignore", str(output)), "Private proof output must be Git ignored")
        package_path = args.input_package.resolve(strict=True)
        package_identity = checked_identity({"bytes": args.input_package_bytes,
                                            "sha256": args.input_package_sha256})
        package, input_path, native_root, native_before = checked_package(package_path, package_identity)
        require(not native_root.is_relative_to(output) and not output.is_relative_to(native_root)
                and not input_path.is_relative_to(output) and not package_path.is_relative_to(output),
                "Private proof output overlaps a provided source")
        source_before = source_snapshot(root, args.revision)
        originals = {"repository": (root, args.revision, source_before),
                     "native": (native_root, NIFLY_REVISION, native_before)}
        driver = Path(__file__).resolve()
        driver_identity = identity(driver)
        receipt["driver"] = {"path": str(driver), **driver_identity}
        receipt["inputs"] = {"package": {"path": str(package_path), **package_identity},
                             "selected": {"path": str(input_path), **identity(input_path)},
                             "geometry": package["geometry"],
                             "absolute_weight_tolerance": package["absolute_weight_tolerance"]}
        inputs = {"package-original": (package_path, package_identity),
                  "selected-original": (input_path, checked_identity(package["input"])),
                  "driver-original": (driver, driver_identity)}
        receipt["source_before"] = {key: value[2] for key, value in originals.items()}
        for name in ("archives", "binaries", "inputs", "target", "native-build"):
            (output / name).mkdir()
        copy_verified(package_path, output / "inputs/input-package.json", package_identity)
        copy_verified(driver, output / "inputs/integration_skin_pose.py", driver_identity)
        selected = output / "inputs" / input_path.name
        require(selected.name.casefold() not in {"input-package.json", "integration_skin_pose.py"},
                "Selected input collides with frozen package/driver")
        copy_verified(input_path, selected, checked_identity(package["input"]))
        inputs.update({"package-frozen": (output / "inputs/input-package.json", package_identity),
                       "selected-frozen": (selected, checked_identity(package["input"])),
                       "driver-frozen": (output / "inputs/integration_skin_pose.py", driver_identity)})
        receipt["inputs_before"] = {key: {"path": str(path), **expected}
                                   for key, (path, expected) in inputs.items()}
        rust_tree, native_tree = output / "source", output / "native-source"
        for name, original, revision, destination in (
                ("rust", root, args.revision, rust_tree),
                ("native", native_root, NIFLY_REVISION, native_tree)):
            snapshot = freeze_repository(commands, original, revision, destination, output / "archives", name)
            frozen[name] = (destination, revision, snapshot)
        lock = document(rust_tree / "sources.lock.json")
        reference = next(row for row in lock["references"] if row["url"] == "https://github.com/ousnius/nifly")
        require(reference["commit"] == NIFLY_REVISION, "Committed native source pin differs")
        receipt["frozen_source_before"] = {key: value[2] for key, value in frozen.items()}
        cargo = args.cargo.resolve(strict=True)
        environment.update(CARGO_HOME=str(args.cargo_home.resolve(strict=True)),
                           RUSTUP_HOME=str(args.rustup_home.resolve(strict=True)),
                           CARGO_TARGET_DIR=str(output / "target"), RUSTUP_AUTO_INSTALL="0")
        environment["PATH"] = str(cargo.parent) + os.pathsep + environment.get("PATH", "")
        retain_binary("cargo", cargo)
        retain_binary("python", Path(sys.executable))
        receipt["toolchain"] = {"cargo": dict(receipt["binaries_before"]["cargo"]),
                                "python": dict(receipt["binaries_before"]["python"]),
                                "cargo_home": environment["CARGO_HOME"],
                                "rustup_home": environment["RUSTUP_HOME"],
                                "target": environment["CARGO_TARGET_DIR"]}
        commands.execute("cargo-version", [cargo, "--version", "--verbose"], rust_tree)
        commands.execute("producer-build", [cargo, "build", "--locked", "--offline", "--release",
                         "--jobs", "1", "-p", "fallout-cli", "--bin", "fallout"], rust_tree)
        built = output / "target/release/fallout.exe"
        cli = output / "binaries/fallout.exe"
        retain_binary("producer-built", built)
        built_identity = receipt["binaries_before"]["producer-built"]
        copy_verified(built, cli, {key: built_identity[key] for key in ("bytes", "sha256")})
        retain_binary("producer-frozen", cli)
        commands.execute("native-build", ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                         rust_tree / "tools/build-nif-skin-oracle.ps1", "-NiflySource", native_tree,
                         "-BuildDirectory", output / "native-build"], rust_tree)
        native_built = output / "native-build/Release/nif-skin-oracle.exe"
        oracle = output / "binaries/nif-skin-oracle.exe"
        retain_binary("native-built", native_built)
        built_identity = receipt["binaries_before"]["native-built"]
        copy_verified(native_built, oracle, {key: built_identity[key] for key in ("bytes", "sha256")})
        retain_binary("native-frozen", oracle)
        native_report = output / "native.json"
        commands.execute("native-source", [sys.executable, rust_tree / "tools/run-nif-skin-oracle.py",
                         "--input", selected, "--binary", oracle, "--include-bindings", "--output", native_report], rust_tree)
        native_identity = identity(native_report)
        native_row = validate_native(native_report, oracle, selected)
        retain_file(commands.evidence, native_report, native_identity)
        comparison = output / "source-comparison.json"
        commands.execute("source-comparison", [cli, "nif-skin", selected, "--include-bindings",
                         "--oracle-report", native_report, "--output", comparison], rust_tree)
        comparison_identity = identity(comparison)
        retain_file(commands.evidence, native_report)
        validate_comparison(comparison, native_report, oracle, selected)
        retain_file(commands.evidence, comparison, comparison_identity)
        pose_directory = output / "pose"
        commands.execute("rational-pose", [sys.executable, rust_tree / "tools/nif-skin-oracle/check_pose.py",
                         "--binary", cli, "--input", selected, "--native-report", native_report,
                         "--geometry", str(package["geometry"]), "--tolerance", str(float(package["absolute_weight_tolerance"])),
                         "--output-dir", pose_directory], rust_tree)
        retain_file(commands.evidence, native_report)
        retain_file(commands.evidence, comparison)
        numeric = validate_pose(pose_directory, cli, selected, native_report, package, native_row, commands.evidence)
        receipt["proof"] = {"native_report": commands.evidence[str(native_report.resolve())],
                            "source_comparison": commands.evidence[str(comparison.resolve())],
                            "pose_summary": commands.evidence[str((pose_directory / "summary.json").resolve())],
                            "numeric": numeric["numeric"],
                            "intended_refusals": 2, "altered_coefficient_rejections": 1}
    except (Exception, KeyboardInterrupt) as error:
        failure = error
    finally:
        receipt["commands"] = commands.rows
        receipt["evidence_before"] = {key: dict(value) for key, value in commands.evidence.items()}
        receipt["source_after"], receipt["frozen_source_after"] = {}, {}
        immutability_errors = []
        for table, key in ((originals, "source_after"), (frozen, "frozen_source_after")):
            for name, (path, revision, before) in table.items():
                try:
                    after = source_snapshot(path, revision)
                    receipt[key][name] = after
                    require(after == before, f"{name} source changed during proof")
                except (Exception, KeyboardInterrupt) as error:
                    immutability_errors.append(f"{key}/{name}: {type(error).__name__}: {error}")
                    failure = failure or error
        receipt["binaries_after"], receipt["inputs_after"] = {}, {}
        for table, key in ((binaries, "binaries_after"),
                           ({name: path for name, (path, _) in inputs.items()}, "inputs_after")):
            for name, path in table.items():
                try:
                    actual = {"path": str(path), **identity(path)}
                    receipt[key][name] = actual
                    before = (receipt["binaries_before"][name] if key == "binaries_after"
                              else {"path": str(path), **inputs[name][1]})
                    require(actual == before, f"{name} changed during proof")
                except (Exception, KeyboardInterrupt) as error:
                    immutability_errors.append(f"{key}/{name}: {type(error).__name__}: {error}")
                    failure = failure or error
        receipt["evidence_after"] = {}
        for name, before in receipt["evidence_before"].items():
            try:
                current = {"path": before["path"], **identity(Path(before["path"]))}
                receipt["evidence_after"][name] = current
                require(current == before, f"Retained evidence changed: {before['path']}")
            except (Exception, KeyboardInterrupt) as error:
                immutability_errors.append(f"evidence_after/{name}: {type(error).__name__}: {error}")
                failure = failure or error
        if immutability_errors:
            receipt["immutability_errors"] = immutability_errors
        receipt["finished_unix"] = time.time()
        receipt["start_receipt"] = receipt["evidence_before"][str((output / "started.json").resolve())]
        receipt["command_receipts"] = [dict(row) for key, row in sorted(receipt["evidence_before"].items())
                                       if Path(key).name.endswith(".command.json")]
        if failure is None:
            receipt.update(state="passed", engineering_proof_passed=True,
                           source_unchanged=True, binaries_unchanged=True, inputs_unchanged=True)
        else:
            receipt["error"] = f"{type(failure).__name__}: {failure}"
        write_json(output / "receipt.json", receipt)
    return 0 if failure is None else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--output-directory", required=True, type=Path)
    parser.add_argument("--input-package", required=True, type=Path)
    parser.add_argument("--input-package-sha256", required=True)
    parser.add_argument("--input-package-bytes", required=True, type=int)
    parser.add_argument("--cargo", required=True, type=Path)
    parser.add_argument("--cargo-home", required=True, type=Path)
    parser.add_argument("--rustup-home", required=True, type=Path)
    args = parser.parse_args()
    try:
        code = run(args)
    except (Exception, KeyboardInterrupt) as error:
        # Refusing an existing/unsafe destination must leave its old receipt alone.
        print(f"skin proof destination refused: {error}", file=sys.stderr)
        return 1
    print(f"Skin engineering proof receipt: {args.output_directory / 'receipt.json'}")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
