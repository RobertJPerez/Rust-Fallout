"""Authored exact animation/native comparisons and deliberate evidence changes."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

from fixtures import STREAMS, container, fixture, w
from process_guard import guard, run as run_process

VARIANTS = ("baseline", "null_links", "unknown_links", "empty_arrays", "unknown_cycle", "all_strings_null", "trailing_nul_strings", "empty_string_table")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, indent=2)
        output.write("\n")


def source(stream, variant):
    result = fixture(stream, variant)
    return result if isinstance(result, bytes) else container(stream, *result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/fallout.exe"))
    parser.add_argument("--oracle", type=Path, default=Path("local/nif-animation-oracle-build/Release/nif-animation-oracle.exe"))
    args = parser.parse_args()
    guard()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    inputs = args.output_dir / "inputs"
    inputs.mkdir()
    for stream in STREAMS:
        for variant in VARIANTS:
            (inputs / f"stream-{stream}-{variant}.blob").write_bytes(source(stream, variant))
    source_hashes = {p.name: digest(p) for p in inputs.iterdir()}
    before = digest(args.oracle)
    rust_before = digest(args.binary)
    oracle_path = args.output_dir / "oracle.json"
    with oracle_path.open("xb") as output:
        result = run_process([str(args.oracle.resolve()), str(inputs.resolve())], stdout=output, stderr=subprocess.PIPE)
    (args.output_dir / "oracle.stderr.txt").write_bytes(result.stderr)
    if result.returncode:
        raise RuntimeError("authored native animation reader failed; retained report/stderr")
    oracle = json.loads(oracle_path.read_text(encoding="utf-8"))
    if before != digest(args.oracle) or oracle.get("oracle_binary_sha256") != before:
        raise AssertionError("native executable before/after/embedded digest differs")
    if len(oracle["files"]) != 96 or not any(row["trailing_nul_normalizations"] > 1 for row in oracle["files"]):
        raise AssertionError("fixture coverage or multiple string supplements missing")

    def run(name, report, directory=inputs):
        command = [str(args.binary.resolve()), "nif-animation", str(directory.resolve()), "--output", str((args.output_dir / f"result-{name}.json").resolve())]
        if report is not None:
            command.extend(["--oracle-report", str(report.resolve())])
        result = run_process(command, capture_output=True)
        (args.output_dir / f"{name}.stdout.txt").write_bytes(result.stdout)
        (args.output_dir / f"{name}.stderr.txt").write_bytes(result.stderr)
        return result.returncode

    if run("valid", oracle_path):
        raise RuntimeError("authored Rust/native animation fields differ; retained result-valid.json")

    def row(document):
        return next(r for r in document["files"] if r["file"] == "stream-34-baseline.blob")

    def data(document, block, key):
        return row(document)["animations"][block]["data"][key]

    mutations = {
        "controller_flags": lambda d: data(d, 0, "controller").update(flags=0),
        "controller_next": lambda d: data(d, 0, "controller").update(next_controller=0),
        "controller_target": lambda d: data(d, 0, "controller").update(target=None),
        "controller_interpolator": lambda d: data(d, 0, "controller").update(interpolator=None),
        "controller_signed_zero": lambda d: data(d, 0, "controller").update(frequency_bits=0),
        "controller_phase": lambda d: data(d, 0, "controller").update(phase_bits=0),
        "controller_start": lambda d: data(d, 0, "controller").update(start_bits=0),
        "controller_stop": lambda d: data(d, 0, "controller").update(stop_bits=0),
        "sequence_name": lambda d: data(d, 1, "sequence").update(name=None),
        "sequence_count": lambda d: data(d, 1, "sequence").update(declared_controlled_blocks=0),
        "sequence_grow": lambda d: data(d, 1, "sequence").update(array_grow_by=0),
        "controlled_order": lambda d: data(d, 1, "sequence")["controlled_blocks"].reverse(),
        "controlled_priority": lambda d: data(d, 1, "sequence")["controlled_blocks"][0].update(priority=0),
        "controlled_name": lambda d: data(d, 1, "sequence")["controlled_blocks"][0].update(node_name=None),
        "sequence_weight": lambda d: data(d, 1, "sequence").update(weight_bits=0),
        "sequence_text_link": lambda d: data(d, 1, "sequence").update(text_keys=None),
        "sequence_cycle": lambda d: data(d, 1, "sequence").update(cycle_type=1),
        "sequence_frequency": lambda d: data(d, 1, "sequence").update(frequency_bits=0),
        "sequence_start": lambda d: data(d, 1, "sequence").update(start_bits=0),
        "sequence_stop": lambda d: data(d, 1, "sequence").update(stop_bits=0),
        "sequence_manager": lambda d: data(d, 1, "sequence").update(manager=0),
        "sequence_accum_name": lambda d: data(d, 1, "sequence").update(accum_root_name=None),
        "note_shape": lambda d: data(d, 1, "sequence")["notes"].update(layout="absent"),
        "note_count": lambda d: data(d, 1, "sequence")["notes"].update(declared_count=0),
        "note_null_position": lambda d: data(d, 1, "sequence")["notes"]["targets"].__setitem__(1, 4),
        "quaternion_order": lambda d: data(d, 2, "interpolator")["rotation_wxyz_bits"].reverse(),
        "translation_sentinel": lambda d: data(d, 2, "interpolator")["translation_bits"].__setitem__(1, 0),
        "scale_sentinel": lambda d: data(d, 2, "interpolator").update(scale_bits=0),
        "transform_data_link": lambda d: data(d, 2, "interpolator").update(data=None),
        "text_name": lambda d: data(d, 3, "text_keys").update(name=None),
        "text_count": lambda d: data(d, 3, "text_keys").update(declared_keys=0),
        "text_order": lambda d: data(d, 3, "text_keys")["keys"].reverse(),
        "text_time": lambda d: data(d, 3, "text_keys")["keys"][1].update(time_bits=0),
        "text_string_link": lambda d: data(d, 3, "text_keys")["keys"][0].update(value=0),
        "raw_trailing_nul": lambda d: row(d)["strings"][8].pop(),
        "raw_non_utf8": lambda d: row(d)["strings"].__setitem__(7, [0]),
        "raw_crlf": lambda d: row(d)["strings"].__setitem__(9, [10]),
        "raw_string_order": lambda d: row(d)["strings"].reverse(),
        "normalization_count": lambda d: row(d).update(trailing_nul_normalizations=0),
        "source_hash": lambda d: row(d).update(sha256="0" * 64),
        "source_length": lambda d: row(d).update(decoded_bytes=0),
        "block_offset": lambda d: row(d)["animations"][0].update(offset=0),
        "block_length": lambda d: row(d)["animations"][0].update(bytes=0),
        "block_hash": lambda d: row(d)["animations"][0].update(sha256="0" * 64),
        "block_id": lambda d: row(d)["animations"][0].update(block=7),
        "block_type": lambda d: row(d)["animations"][0].update(block_type="NiTimeController"),
        "block_omission": lambda d: row(d)["animations"].pop(),
        "file_omission": lambda d: d["files"].pop(),
        "file_duplicate": lambda d: d["files"].append(copy.deepcopy(d["files"][0])),
        "tuple": lambda d: row(d).update(bethesda_version=100),
        "source_pin": lambda d: d.update(nifly_revision="0" * 40),
        "binary_hash": lambda d: d.update(oracle_binary_sha256=None),
        "raw_string_contract": lambda d: d.update(raw_string_table_checked=False),
        "raw_count_contract": lambda d: d.update(raw_count_fields_checked=False),
        "preparation_called": lambda d: d.update(prepare_data_called=True),
        "branch": lambda d: d.update(animation_branch="other"),
        "string_encoding": lambda d: d.update(string_encoding="utf-8"),
        "float_encoding": lambda d: d.update(float_encoding="float"),
        "runtime_claim": lambda d: d.update(runtime_ready=True),
    }
    corruptions = []
    for name, mutate in mutations.items():
        altered = copy.deepcopy(oracle)
        mutate(altered)
        path = args.output_dir / f"altered-{name}.json"
        write(path, altered)
        code = run(name, path)
        if code == 0:
            raise AssertionError(f"altered {name} evidence was accepted")
        corruptions.append({"name": name, "exit_code": code, "rejected": True})

    bad_sources = []
    for name, block, offset, replacement in [("controller_nan", 0, 6, w(0x7FC00001)), ("sequence_count", 1, 4, w(0xFFFFFFFF)), ("text_count", 3, 4, w(0xFFFFFFFF)), ("note_count", 1, 102, b"\xff\xff"), ("reference_range", 0, 26, w(8)), ("string_range", 3, 12, w(10)), ("interpolator_surplus", 2, 36, b"\0")]:
        blocks, strings = fixture(34, "baseline")
        payload = bytearray(blocks[block][1])
        payload[offset:offset + len(replacement)] = replacement
        blocks[block] = (blocks[block][0], bytes(payload))
        directory = args.output_dir / f"bad-{name}"
        directory.mkdir()
        path = directory / "invalid.blob"
        path.write_bytes(container(34, blocks, strings))
        report = args.output_dir / f"bad-{name}-oracle.json"
        with report.open("xb") as output:
            native = run_process([str(args.oracle.resolve()), str(directory.resolve())], stdout=output, stderr=subprocess.PIPE)
        native_document = json.loads(report.read_text())
        rust = run(f"bad-source-{name}", None, directory)
        if native.returncode == 0 or rust == 0 or not native_document["files"][0].get("error"):
            raise AssertionError(f"malformed {name} source was accepted or native error JSON malformed")
        bad_sources.append({"name": name, "native_exit": native.returncode, "rust_exit": rust, "rejected": True})
    if digest(args.oracle) != before or digest(args.binary) != rust_before or source_hashes != {p.name: digest(p) for p in inputs.iterdir()}:
        raise AssertionError("comparison executables or original fixture inputs changed")
    summary = {"schema_version": 1, "fixtures": 96, "streams": list(STREAMS), "variants": list(VARIANTS), "deliberate_report_changes": corruptions, "malformed_sources": bad_sources, "cli_sha256": rust_before, "oracle_sha256_before_after_embedded": before, "oracle_report_sha256": digest(oracle_path), "rust_report_sha256": digest(args.output_dir / "result-valid.json"), "gameplay_accepted": False}
    write(args.output_dir / "summary.json", summary)
    print(f"96 exact animation source files; {len(corruptions)} changed reports and {len(bad_sources)} malformed sources rejected")


if __name__ == "__main__":
    main()
