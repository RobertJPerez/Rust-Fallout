"""Exercise full-cohort CLI requests with preserved authored trace inputs."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def document(path: Path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--authored-input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    inputs = args.authored_input.resolve(strict=True)
    output = args.output.resolve()
    if output == inputs or inputs in output.parents:
        raise ValueError("Keep output outside the preserved input package")
    output.mkdir(parents=False, exist_ok=False)
    original = {path: digest(path) for path in inputs.rglob("*") if path.is_file()}
    binary_hash = digest(binary)
    manifest = document(inputs / "manifest.json")
    command = [str(binary), "source-plans", "--install",
               str(inputs / "authored-source-copy"), "--load-order",
               str(inputs / "order.json")]
    results = []

    def invoke(name, arguments):
        process = subprocess.run(command + arguments, capture_output=True, check=False)
        (output / (name + ".stdout")).write_bytes(process.stdout)
        (output / (name + ".stderr")).write_bytes(process.stderr)
        results.append({"name": name, "command": command + arguments,
                        "exit_code": process.returncode})
        return process

    # A report can contain unresolved source findings and still export its exact
    # identity. We test that identity through admission, not the CLI exit alone.
    report_path = output / "sources.json"
    source_run = invoke("sources", ["--output", str(report_path)])
    require(source_run.returncode in (0, 1) and report_path.is_file(), "Source report was not produced")
    sources = document(report_path)
    full = sources["prepared_source_cohort_sha256"]
    legacy = sources["source_cohort_sha256"]
    require(full == manifest["identity"]["source_cohort_sha256"], "Full cohort differs from preserved trace input")
    require(legacy == sources["winning_definitions_sha256"] and legacy != full, "Digest domains were conflated")
    require(sources["source_cohort_sha256_domain"] == "legacy_winning_definitions", "Legacy digest domain is missing")
    handle = manifest["identity"]["definition"]
    require(any(row["handle"] == handle for row in sources["definitions"]), "Selected handle is absent")
    for name, cohort, admitted in (
        ("full-cohort", full, True),
        ("legacy-cohort", legacy, False),
        ("altered-cohort", "0" * 64, False),
    ):
        request = output / (name + ".request.json")
        request.write_text(json.dumps({"schema_version": 1,
                                      "source_cohort_sha256": cohort,
                                      "roots": [handle]}, indent=2) + "\n",
                           encoding="utf-8")
        path = output / (name + ".report.json")
        process = invoke(name, ["--execution-admission", str(request),
                                "--output", str(path)])
        require(process.returncode == 1, f"{name}: unexpected CLI result")
        if admitted:
            report = document(path)
            graph = report["execution_admission"]
            require(graph["source_cohort_sha256"] == full and graph["roots"] == [handle], "Admission identity differs")
            require(graph["first_unsupported"]["code"] == "unverified_assignment", "Expected authored assignment finding missing")
            require(graph["first_unsupported"]["source_scda_offset"] == 10, "Authored assignment source offset differs")
            require(graph["faithful_execution_admitted"] is False, "Engineering finding granted execution")
            require(report["retail_parity_accepted"] is False, "Engineering request claimed retail acceptance")
        else:
            require(not path.exists(), "Rejected cohort produced a partial report")
            require(b"different source receipt cohort" in process.stderr, "Cohort refusal diagnostic missing")
    require(digest(binary) == binary_hash, "Frozen producer changed")
    require(original == {path: digest(path) for path in inputs.rglob("*") if path.is_file()}, "Preserved source package changed")
    receipt = {"schema_version": 1, "binary_sha256": binary_hash,
               "manifest_sha256": digest(inputs / "manifest.json"),
               "full_cohort_sha256": full, "legacy_cohort_sha256": legacy,
               "commands": results, "source_and_binary_unchanged": True,
               "faithful_execution_admitted": False, "original_process_started": False}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
