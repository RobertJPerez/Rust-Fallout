"""Authored schema-2 partition fixtures, exact native comparison and rejections."""
import argparse
import copy
import json
from pathlib import Path
import struct
import subprocess

from check_comparison import STREAMS, digest, fixture, shorts, words, write_json

BRANCH = "nv-canonical-flags-four-wide-or-empty"


def replace_partition(source, payload):
    """Change an authored block using only independently read container framing."""
    source = bytearray(source)
    start = source.index(b"\n") + 1
    blocks = struct.unpack_from("<I", source, start + 9)[0]
    offset = start + 20
    types = struct.unpack_from("<H", source, offset)[0]
    offset += 2
    for _ in range(types):
        length = struct.unpack_from("<I", source, offset)[0]
        offset += 4 + length
    offset += 2 * blocks
    sizes_at = offset
    sizes = struct.unpack_from("<" + "I" * blocks, source, sizes_at)
    offset += 4 * blocks + 12  # fixture has no strings/groups
    block_at = offset + sum(sizes[:5])
    struct.pack_into("<I", source, sizes_at + 5 * 4, len(payload))
    source[block_at:block_at + sizes[5]] = payload
    return bytes(source)


def packet(strips=False):
    result = shorts(3, 42 if strips else 1, 2, int(strips), 4, 1, 0)
    result += bytes([1]) + shorts(2, 0, 1) + bytes([1])
    result += words(0x80000000, 0xBE800000, 0x3FA00000, 0x3F800000) * 3
    if strips:
        result += shorts(4)
    result += bytes([1]) + (shorts(2, 0, 1, 1) if strips else shorts(2, 0, 1))
    return result + bytes([1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0])


def payload(packets):
    return words(len(packets)) + b"".join(packets)


def run_native(binary, source, output):
    before = digest(binary)
    with output.open("xb") as stream:
        result = subprocess.run([str(binary.resolve()), str(source.resolve()), "--include-partitions"],
                                stdout=stream, stderr=subprocess.PIPE)
    after = digest(binary)
    document = json.loads(output.read_text(encoding="utf-8"))
    if before != after or document.get("oracle_binary_sha256") != before:
        raise AssertionError("native binary changed or embedded hash differs")
    return result, document, before


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/fallout.exe"))
    parser.add_argument("--oracle", type=Path, default=Path("local/nif-skin-oracle-build-02/Release/nif-skin-oracle.exe"))
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    inputs = args.output_dir / "inputs"
    inputs.mkdir()
    absent = shorts(3, 9, 2, 2, 17, 1, 0) + bytes([0, 0]) + shorts(6, 8) + bytes([0, 0])
    empty = shorts(0, 0, 0, 0, 65535) + bytes([1] * 4)
    variants = {"triangle": [packet()], "strip": [packet(True)], "absent": [absent],
                "empty": [empty], "zero": [], "mixed": [packet(), packet(True), absent, empty]}
    for stream in STREAMS:
        for ordinal, (name, packets) in enumerate(variants.items()):
            flag = (0, 1, 2, 255)[ordinal % 4]
            (inputs / f"stream-{stream}-{name}.blob").write_bytes(
                replace_partition(fixture(stream, flag, name != "triangle"), payload(packets)))
    oracle_path = args.output_dir / "oracle.json"
    native, oracle, oracle_hash = run_native(args.oracle, inputs, oracle_path)
    (args.output_dir / "oracle.stderr.txt").write_bytes(native.stderr)
    if native.returncode:
        raise RuntimeError("native fixtures failed; retained output/stderr")
    for row in oracle["files"]:
        kind = row["file"].removesuffix(".blob").split("-")[-1]
        parts = row["partitions"][0]["partitions"]
        if len(parts) != len(variants[kind]):
            raise AssertionError("authored partition count changed")
        if kind in ("triangle", "strip", "mixed"):
            part = parts[0]
            assert part["bone_palette"] == [1, 0] and part["vertex_map"] == [2, 0, 1]
            assert part["weight_bits"] == [0x80000000, 0xBE800000, 0x3FA00000, 0x3F800000] * 3
            assert part["bone_indices"] == [1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0]
        if kind == "strip":
            assert parts[0]["num_triangles"] == 42 and parts[0]["strips"] == [[2, 0, 1, 1]]
            assert parts[0]["triangles"] == []
        if kind == "absent":
            assert parts[0]["weights_per_vertex"] == 17 and parts[0]["strip_lengths"] == [6, 8]
            assert parts[0]["weight_bits"] == [] and parts[0]["has_faces"] == 0
        if kind == "empty":
            assert parts[0]["weights_per_vertex"] == 65535 and parts[0]["has_vertex_weights"] == 1

    def run(name, expected, source=inputs):
        result = subprocess.run([str(args.binary.resolve()), "nif-skin", str(source.resolve()),
                                 "--include-partitions", "--oracle-report", str(expected.resolve()),
                                 "--output", str((args.output_dir / f"rust-{name}.json").resolve())],
                                capture_output=True)
        (args.output_dir / f"{name}.stdout.txt").write_bytes(result.stdout)
        (args.output_dir / f"{name}.stderr.txt").write_bytes(result.stderr)
        return result.returncode

    if run("valid", oracle_path):
        raise RuntimeError("native/Rust authored partition fields differ")
    triangle_row = next(i for i, row in enumerate(oracle["files"]) if row["file"].endswith("-triangle.blob"))
    strip_row = next(i for i, row in enumerate(oracle["files"]) if row["file"].endswith("-strip.blob"))
    mutations = {}
    for field in ("num_vertices", "num_triangles", "num_bones", "num_strips", "weights_per_vertex",
                  "has_vertex_map", "has_vertex_weights", "has_faces", "has_bone_indices"):
        mutations[field] = lambda obj, field=field: obj["files"][triangle_row]["partitions"][0]["partitions"][0].update({field: 65535})
    for field in ("bone_palette", "vertex_map", "weight_bits", "bone_indices"):
        mutations[field] = lambda obj, field=field: obj["files"][triangle_row]["partitions"][0]["partitions"][0][field].reverse()
    mutations["triangles"] = lambda obj: obj["files"][triangle_row]["partitions"][0]["partitions"][0]["triangles"][0].reverse()
    mutations["strip_lengths"] = lambda obj: obj["files"][strip_row]["partitions"][0]["partitions"][0].update(strip_lengths=[3])
    mutations["strips"] = lambda obj: obj["files"][strip_row]["partitions"][0]["partitions"][0]["strips"][0].reverse()
    for field, value in (("sha256", "0" * 64), ("offset", 0), ("bytes", 0), ("block", 0), ("block_type", "NiSkinData")):
        mutations["block_" + field] = lambda obj, field=field, value=value: obj["files"][triangle_row]["partitions"][0].update({field: value})
    mutations.update({
        "partition_omission": lambda obj: obj["files"][triangle_row]["partitions"].clear(),
        "packet_omission": lambda obj: obj["files"][triangle_row]["partitions"][0]["partitions"].clear(),
        "owner_vertex_count": lambda obj: obj["files"][triangle_row]["owners"][0].update(vertex_count=4),
        "skin_root": lambda obj: obj["files"][triangle_row]["skins"][1]["data"]["instance"].update(skeleton_root=None),
        "schema": lambda obj: obj.update(schema_version=1),
        "branch": lambda obj: obj.update(partition_branch="unverified"),
        "raw_contract": lambda obj: obj.update(raw_partition_fields_checked=False),
        "binary_hash": lambda obj: obj.update(oracle_binary_sha256=None),
    })
    checks = []
    for name, mutate in mutations.items():
        altered = copy.deepcopy(oracle); mutate(altered)
        path = args.output_dir / f"altered-{name}.json"; write_json(path, altered)
        code = run(name, path)
        if code == 0:
            raise AssertionError(f"deliberate {name} mismatch accepted")
        checks.append({"name": name, "exit_code": code, "rejected": True})
    # Unsupported source branches must fail before upstream bool/array loading.
    rejected = []
    for name, offset, data in (("flag", 14, bytes([2])), ("width", 8, shorts(3)),
                               ("truncated", None, None), ("surplus", None, None)):
        source = bytearray(payload([packet()]))
        if offset is not None: source[4 + offset:4 + offset + len(data)] = data
        elif name == "truncated": source = source[:-1]
        else: source += bytes([0])
        path = args.output_dir / f"rejected-{name}.blob"
        path.write_bytes(replace_partition(fixture(34, 1, False), source))
        result, _, _ = run_native(args.oracle, path, args.output_dir / f"rejected-{name}-oracle.json")
        if result.returncode == 0: raise AssertionError(f"native accepted unsupported/malformed {name}")
        rust = subprocess.run([str(args.binary.resolve()), "nif-skin", str(path.resolve()), "--include-partitions",
                               "--output", str((args.output_dir / f"rejected-{name}-rust.json").resolve())], capture_output=True)
        if rust.returncode == 0: raise AssertionError(f"Rust accepted unsupported/malformed {name}")
        rejected.append({"name": name, "native_exit": result.returncode, "rust_exit": rust.returncode})
    summary = {"schema_version": 2, "partition_branch": BRANCH, "fixtures": len(STREAMS) * len(variants),
               "stream_revisions": list(STREAMS), "deliberate_mismatches": checks, "source_rejections": rejected,
               "oracle_sha256_before_after_embedded": oracle_hash, "cli_sha256": digest(args.binary),
               "oracle_report_sha256": digest(oracle_path), "rust_report_sha256": digest(args.output_dir / "rust-valid.json"),
               "gameplay_accepted": False}
    write_json(args.output_dir / "summary.json", summary)
    print(f"{summary['fixtures']} exact authored comparisons; {len(checks)} altered reports and {len(rejected)} source branches rejected")


if __name__ == "__main__":
    main()
