"""Authored skin fixtures and deliberate corruption checks through the real CLI."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess

NULL = 0xFFFFFFFF
STREAMS = (14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34)


def words(*values):
    return struct.pack("<" + "I" * len(values), *values)


def shorts(*values):
    return struct.pack("<" + "H" * len(values), *values)


def floats(*values):
    return struct.pack("<" + "f" * len(values), *values)


def transform():
    return floats(1, -0.0, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0.5)


def fixture(stream, flag, dismember):
    av = words(NULL, 0, NULL) + (shorts(14) if stream <= 26 else words(14))
    av += floats(0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1) + words(0, NULL)
    node = av + words(0, 0)
    data = transform() + words(2) + bytes([flag])
    for vertex, count, weight in [(0, 7, -0.0), (2, 19, 1.25)]:
        data += transform() + floats(13, 14, 15, 16) + shorts(1 if flag else count)
        if flag:
            data += shorts(vertex) + floats(weight)
    instance = words(1, 5, 0, 2, 0, NULL)
    if dismember:
        instance += words(2) + shorts(0x8101, 65000, 0xFFFF, 201)
    geometry = words(0) + shorts(3) + bytes([0, 0, 0]) + shorts(0) + bytes([0])
    geometry += floats(0, 0, 0, 1) + bytes([0]) + shorts(0) + words(NULL)
    geometry += shorts(0) + words(0) + bytes([0]) + shorts(0)
    shape = av + words(3, 2, 0, NULL) + bytes([0])
    blocks = [("NiNode", node), ("NiSkinData", data),
              ("BSDismemberSkinInstance" if dismember else "NiSkinInstance", instance),
              ("NiTriShapeData", geometry), ("NiTriShape", shape),
              ("NiSkinPartition", b""), ("NiTriShape", shape)]
    names = list(dict.fromkeys(name for name, _ in blocks))
    result = b"Gamebryo File Format, Version 20.2.0.7\n" + words(0x14020007)
    result += bytes([1]) + words(11, len(blocks), stream) + bytes(3) + shorts(len(names))
    result += b"".join(words(len(name)) + name.encode("ascii") for name in names)
    result += b"".join(shorts(names.index(name)) for name, _ in blocks)
    result += b"".join(words(len(payload)) for _, payload in blocks)
    return result + words(0, 0, 0) + b"".join(payload for _, payload in blocks) + words(0)


def write_json(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, indent=2)
        output.write("\n")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/fallout.exe"))
    parser.add_argument("--oracle", type=Path, default=Path("local/nif-skin-oracle-build/Release/nif-skin-oracle.exe"))
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    inputs = args.output_dir / "inputs"
    inputs.mkdir()
    for stream in STREAMS:
        for flag in (0, 1, 2, 255):
            (inputs / f"stream-{stream}-flag-{flag}.blob").write_bytes(fixture(stream, flag, flag != 1))
    oracle_path = args.output_dir / "oracle.json"
    before = digest(args.oracle)
    with oracle_path.open("xb") as output:
        native = subprocess.run([str(args.oracle.resolve()), str(inputs.resolve())], stdout=output, stderr=subprocess.PIPE)
    (args.output_dir / "oracle.stderr.txt").write_bytes(native.stderr)
    if native.returncode:
        raise RuntimeError("native fixture oracle failed; retained oracle.json and stderr")
    oracle = json.loads(oracle_path.read_text(encoding="utf-8"))
    if before != digest(args.oracle) or oracle.get("oracle_binary_sha256") != before:
        raise AssertionError("oracle binary changed or embedded hash differs")
    if sum(row["presence_normalizations"] for row in oracle["files"]) != 24:
        raise AssertionError("presence normalization supplements were not exercised")
    if sum(row["vertex_count_normalizations"] for row in oracle["files"]) != 24:
        raise AssertionError("absent-weight count supplements were not exercised")

    def run(name, expected, output):
        result = subprocess.run([str(args.binary.resolve()), "nif-skin", str(inputs.resolve()),
                                 "--oracle-report", str(expected.resolve()), "--output", str(output.resolve())],
                                capture_output=True)
        (args.output_dir / f"{name}.stdout.txt").write_bytes(result.stdout)
        (args.output_dir / f"{name}.stderr.txt").write_bytes(result.stderr)
        return result.returncode

    rust_path = args.output_dir / "rust.json"
    if run("valid", oracle_path, rust_path):
        raise RuntimeError("Rust/native fixture fields differ; see rust.json")
    checks = []
    mutations = {
        "weight_bits": lambda obj: obj["files"][1]["skins"][0]["data"]["bones"][0]["weights"][0].update(weight_bits=0),
        "weight_vertex_index": lambda obj: obj["files"][1]["skins"][0]["data"]["bones"][0]["weights"][0].update(vertex=1),
        "bounding_radius": lambda obj: obj["files"][0]["skins"][0]["data"]["bones"][0].update(radius_bits=0),
        "transform_negative_zero": lambda obj: obj["files"][0]["skins"][0]["data"]["transform"]["rotation_bits"][0].__setitem__(1, 0),
        "presence_byte": lambda obj: obj["files"][0]["skins"][0]["data"].update(has_vertex_weights=1),
        "absent_vertex_count": lambda obj: obj["files"][0]["skins"][0]["data"]["bones"][0].update(declared_vertices=0),
        "reference_order": lambda obj: obj["files"][0]["skins"][1]["data"]["instance"]["bones"].reverse(),
        "dismember_flags": lambda obj: obj["files"][0]["skins"][1]["data"]["instance"]["body_parts"][0].update(flags=0),
        "body_part_id": lambda obj: obj["files"][0]["skins"][1]["data"]["instance"]["body_parts"][0].update(body_part=0),
        "source_hash": lambda obj: obj["files"][0].update(sha256="0" * 64),
        "source_length": lambda obj: obj["files"][0].update(decoded_bytes=0),
        "block_hash": lambda obj: obj["files"][0]["skins"][0].update(sha256="0" * 64),
        "block_offset": lambda obj: obj["files"][0]["skins"][0].update(offset=0),
        "block_length": lambda obj: obj["files"][0]["skins"][0].update(bytes=0),
        "block_omission": lambda obj: obj["files"][0]["skins"].pop(),
        "owner_omission": lambda obj: obj["files"][0]["owners"].pop(),
        "owner_vertex_count": lambda obj: obj["files"][0]["owners"][0].update(vertex_count=4),
        "tuple": lambda obj: obj["files"][0].update(bethesda_version=100),
        "file_omission": lambda obj: obj["files"].pop(),
        "duplicate_file": lambda obj: obj["files"].append(copy.deepcopy(obj["files"][0])),
        "normalization_contract": lambda obj: obj.update(raw_presence_and_vertex_counts_checked=False),
        "prepare_data": lambda obj: obj.update(prepare_data_called=True),
        "source_pin": lambda obj: obj.update(nifly_revision="0" * 40),
        "binary_hash": lambda obj: obj.update(oracle_binary_sha256=None),
    }
    for name, mutate in mutations.items():
        altered = copy.deepcopy(oracle)
        mutate(altered)
        path = args.output_dir / f"altered-{name}.json"
        write_json(path, altered)
        code = run(name, path, args.output_dir / f"result-{name}.json")
        if code == 0:
            raise AssertionError(f"deliberate {name} corruption was accepted")
        checks.append({"name": name, "exit_code": code, "rejected": True})
    result = {"schema_version": 1, "fixtures": 48, "stream_revisions": list(STREAMS),
              "presence_normalizations_checked": 24, "vertex_count_normalizations_checked": 24,
              "deliberate_mismatches": checks, "cli_sha256": digest(args.binary),
              "oracle_sha256": digest(args.oracle), "oracle_report_sha256": digest(oracle_path),
              "oracle_sha256_before_after_embedded": before,
              "rust_report_sha256": digest(rust_path), "gameplay_accepted": False}
    write_json(args.output_dir / "summary.json", result)
    print(f"48 exact fixture comparisons; {len(checks)} deliberately altered reports rejected")


if __name__ == "__main__":
    main()
