"""Independent authored source graphs and deliberate schema-3 report corruptions."""
import argparse
import copy
import json
from pathlib import Path
import subprocess

from check_comparison import NULL, STREAMS, digest, floats, shorts, transform, words, write_json


def container(blocks, stream, roots, strings=()):
    names = list(dict.fromkeys(name for name, _ in blocks))
    out = b"Gamebryo File Format, Version 20.2.0.7\n" + words(0x14020007) + bytes([1])
    out += words(11, len(blocks), stream) + bytes(3) + shorts(len(names))
    out += b"".join(words(len(name)) + name.encode("ascii") for name in names)
    out += b"".join(shorts(names.index(name)) for name, _ in blocks)
    out += b"".join(words(len(data)) for _, data in blocks)
    out += words(len(strings), max((len(s) for s in strings), default=0))
    out += b"".join(words(len(s)) + s.encode("ascii") for s in strings) + words(0)
    return out + b"".join(data for _, data in blocks) + words(len(roots)) + words(*roots)


def av(stream, name=NULL):
    return (words(name, 0, NULL) + (shorts(65535) if stream <= 26 else words(0xFFFFFFFF))
            + floats(1, -0.0, 3, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0.5) + words(0, NULL))


def node(stream, children=(), effects=(), name=NULL):
    return av(stream, name) + words(len(children)) + words(*children) + words(len(effects)) + words(*effects)


def instance(root, bones, data=3, partition=8):
    return words(data, partition, root, len(bones)) + words(*bones)


def shape(stream, skin=4, data=5):
    return av(stream) + words(data, skin, 0, NULL) + bytes([0])


def geometry():
    return (words(0) + shorts(0) + bytes(3) + shorts(0) + bytes([0]) + floats(0, 0, 0, 1)
            + bytes([0]) + shorts(0) + words(NULL) + shorts(0) + words(0) + bytes([0]) + shorts(0))


def blocks_for(stream, variant):
    # No authored weights are present. Nonzero absent counts must remain raw.
    data = transform() + words(3) + bytes([0])
    data += b"".join(transform() + floats(0, 0, 0, 1) + shorts(count) for count in (9, 8, 7))
    bones = [2, 1, 2]
    root = 0
    roots = [0]
    strings = []
    blocks = [("NiNode", node(stream, [1, 6, 7])), ("BSFadeNode", node(stream, [2])),
              ("NiNode", node(stream)), ("NiSkinData", data),
              ("NiSkinInstance", instance(root, bones)), ("NiTriShapeData", geometry()),
              ("NiTriShape", shape(stream)), ("NiTriShape", shape(stream)),
              ("NiSkinPartition", words(0))]
    if variant == "root_self": bones = [2, 0, 2]
    elif variant == "null_links": root, bones = NULL, [NULL, 1, NULL]
    elif variant == "null_bone": bones = [2, NULL, 2]
    elif variant == "unowned": blocks[6] = ("NiTriShape", shape(stream, NULL)); blocks[7] = blocks[6]
    elif variant == "outside": blocks[1] = ("BSFadeNode", node(stream)); roots = [0, 2]
    elif variant == "no_footer": roots = []
    elif variant == "unknown_bone": blocks[2] = ("NiBillboardNode", node(stream) + shorts(0))
    elif variant == "unknown_root": blocks[0] = ("NiBillboardNode", node(stream, [1, 6, 7]) + shorts(0))
    elif variant == "unknown_intermediate":
        blocks[0] = ("NiNode", node(stream, [9, 6, 7]))
        blocks.append(("NiBillboardNode", node(stream, [1]) + shorts(0)))
    elif variant == "duplicate_names":
        strings = ["DuplicateBoneName"]
        blocks[0] = ("NiNode", node(stream, [1, 6, 7], name=0))
        blocks[1] = ("BSFadeNode", node(stream, [2], name=0)); blocks[2] = ("NiNode", node(stream, name=0))
    elif variant == "null_arrays":
        blocks[0] = ("NiNode", node(stream, [NULL, 1, NULL, 6, 7], [NULL, 1, NULL])); roots = [NULL, 0, NULL]
    elif variant == "secondary_root": root = 1
    elif variant == "shared_instance":
        blocks.extend([("NiSkinInstance", instance(1, [1, 2, 1])), ("NiTriShape", shape(stream, 9))])
        blocks[0] = ("NiNode", node(stream, [1, 6, 7, 10]))
    blocks[4] = ("NiSkinInstance", instance(root, bones))
    return blocks, roots, strings, bones


def native(binary, source, output):
    before = digest(binary)
    with output.open("xb") as stream:
        result = subprocess.run([str(binary.resolve()), str(source.resolve()), "--include-bindings"],
                                stdout=stream, stderr=subprocess.PIPE)
    document = json.loads(output.read_text(encoding="utf-8"))
    if before != digest(binary) or document.get("oracle_binary_sha256") != before:
        raise AssertionError("native binary changed or embedded digest differs")
    return result, document, before


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/fallout.exe"))
    parser.add_argument("--oracle", type=Path, default=Path("local/nif-skin-oracle-build-03/Release/nif-skin-oracle.exe"))
    args = parser.parse_args(); args.output_dir.mkdir(parents=True, exist_ok=False)
    inputs = args.output_dir / "inputs"; inputs.mkdir()
    variants = ("nested", "root_self", "null_links", "null_bone", "unowned", "outside", "no_footer",
                "unknown_bone", "unknown_root", "unknown_intermediate", "duplicate_names", "null_arrays",
                "secondary_root", "shared_instance")
    expected = {}
    for stream in STREAMS:
        for variant in variants:
            blocks, roots, strings, bones = blocks_for(stream, variant)
            name = f"stream-{stream}-{variant}.blob"; (inputs / name).write_bytes(container(blocks, stream, roots, strings))
            expected[name] = (variant, bones)
    # Deep authored chain verifies iterative Rust graph access and separately
    # bounded native ancestor walks, without either implementation's pose math.
    depth = 10000
    blocks = [("NiNode", node(34, [i + 1 if i + 1 < depth else depth + 3])) for i in range(depth)]
    blocks.extend([("NiSkinInstance", instance(0, [depth - 1, 0, depth - 1], NULL, depth + 1)),
                   ("NiSkinPartition", words(0)), ("NiTriShapeData", geometry()),
                   ("NiTriShape", shape(34, depth, depth + 2))])
    (inputs / "deep-10000.blob").write_bytes(container(blocks, 34, [0])); expected["deep-10000.blob"] = ("deep", [depth - 1, 0, depth - 1])
    oracle_path = args.output_dir / "oracle.json"; result, oracle, oracle_hash = native(args.oracle, inputs, oracle_path)
    (args.output_dir / "oracle.stderr.txt").write_bytes(result.stderr)
    if result.returncode: raise RuntimeError("native source graph fixtures failed; retained output")
    for row in oracle["files"]:
        variant, bones = expected[row["file"]]; bindings = row["bindings"]; inst = bindings["instances"][0]
        assert bindings["ancestry_scope"] == "decoded-source-forest"
        assert [b["node"]["target"] for b in inst["bones"]] == [None if b == NULL else b for b in bones]
        assert [b["ordinal"] for b in inst["bones"]] == [0, 1, 2]
        if variant in ("nested", "root_self", "deep", "duplicate_names", "null_arrays", "shared_instance", "secondary_root"):
            assert all(b["decoded_root_contains"] is True for b in inst["bones"])
        if variant in ("outside", "unknown_intermediate"):
            assert inst["bones"][0]["decoded_root_contains"] is False
        if variant == "unknown_intermediate": assert bindings["unsupported_scene_edges"][0]["target"] == 9
        if variant in ("unknown_root", "null_links"):
            assert all(b["decoded_root_contains"] is None for b in inst["bones"])
        if variant == "unknown_bone": assert inst["bones"][0]["decoded_root_contains"] is None
        if variant == "unowned": assert inst["owners"] == []
        if variant == "no_footer": assert inst["bones"][0]["node"]["reachable_from_footer"] is False
        if variant == "duplicate_names": assert all(n["name"] == 0 for n in bindings["nodes"])
        if variant == "null_arrays":
            assert bindings["nodes"][0]["children"] == [None, 1, None, 6, 7]
            assert bindings["nodes"][0]["effects"] == [None, 1, None] and bindings["footer_roots"] == [None, 0, None]

    def rust(name, oracle_file):
        result = subprocess.run([str(args.binary.resolve()), "nif-skin", str(inputs.resolve()), "--include-bindings",
                                 "--oracle-report", str(oracle_file.resolve()), "--output",
                                 str((args.output_dir / f"rust-{name}.json").resolve())], capture_output=True)
        (args.output_dir / f"{name}.stderr.txt").write_bytes(result.stderr)
        return result.returncode

    if rust("valid", oracle_path): raise RuntimeError("Rust/native authored graph fields differ")
    nested = next(i for i, row in enumerate(oracle["files"]) if row["file"].endswith("-nested.blob"))
    unknown = next(i for i, row in enumerate(oracle["files"]) if row["file"].endswith("-unknown_intermediate.blob"))
    mutations = {}
    for field, value in (("block", 99), ("block_type", "NiBillboardNode"), ("offset", 0), ("bytes", 0),
                         ("sha256", "0" * 64), ("name", 0), ("controller", 1), ("flags", 0),
                         ("parent", 1), ("reachable_from_footer", False), ("extra_data", [None]),
                         ("properties", [None]), ("collision", 1), ("effects", [None])):
        mutations["node_" + field] = lambda obj, f=field, v=value: obj["files"][nested]["bindings"]["nodes"][0].update({f: v})
    mutations.update({
        "node_signed_zero": lambda o: o["files"][nested]["bindings"]["nodes"][0]["transform"]["translation_bits"].__setitem__(1, 0),
        "children_order": lambda o: o["files"][nested]["bindings"]["nodes"][0]["children"].reverse(),
        "node_omission": lambda o: o["files"][nested]["bindings"]["nodes"].pop(),
        "footer_position": lambda o: o["files"][nested]["bindings"]["footer_roots"].append(None),
        "bone_target": lambda o: o["files"][nested]["bindings"]["instances"][0]["bones"][0]["node"].update(target=1),
        "bone_ordinal": lambda o: o["files"][nested]["bindings"]["instances"][0]["bones"][0].update(ordinal=1),
        "bone_decoded": lambda o: o["files"][nested]["bindings"]["instances"][0]["bones"][0]["node"].update(decoded_node=False),
        "bone_parent": lambda o: o["files"][nested]["bindings"]["instances"][0]["bones"][0]["node"].update(parent=0),
        "bone_membership": lambda o: o["files"][nested]["bindings"]["instances"][0]["bones"][0].update(decoded_root_contains=False),
        "bone_reachability": lambda o: o["files"][nested]["bindings"]["instances"][0]["bones"][0]["node"].update(reachable_from_footer=False),
        "owner_order": lambda o: o["files"][nested]["bindings"]["instances"][0]["owners"].reverse(),
        "owner_membership": lambda o: o["files"][nested]["bindings"]["instances"][0]["owners"][0].update(decoded_root_contains=False),
        "owner_parent": lambda o: o["files"][nested]["bindings"]["instances"][0]["owners"][0].update(parent=1),
        "root_target": lambda o: o["files"][nested]["bindings"]["instances"][0]["skeleton_root"].update(target=1),
        "instance_omission": lambda o: o["files"][nested]["bindings"]["instances"].clear(),
        "unknown_edge": lambda o: o["files"][unknown]["bindings"]["unsupported_scene_edges"][0].update(target=1),
        "scope": lambda o: o.update(binding_scope="complete-source"),
        "raw_contract": lambda o: o.update(raw_node_fields_checked=False),
        "graph_contract": lambda o: o.update(graph_membership_checked=False),
        "schema": lambda o: o.update(schema_version=2),
        "binary_hash": lambda o: o.update(oracle_binary_sha256=None),
    })
    checks = []
    for name, mutate in mutations.items():
        altered = copy.deepcopy(oracle); mutate(altered); path = args.output_dir / f"altered-{name}.json"; write_json(path, altered)
        code = rust(name, path)
        if code == 0: raise AssertionError(f"deliberate graph corruption {name} accepted")
        checks.append({"name": name, "exit_code": code, "rejected": True})
    write_json(args.output_dir / "summary.json", {"schema_version": 3, "fixtures": len(expected), "stream_revisions": list(STREAMS),
               "ancestry_scope": "decoded-source-forest", "deep_nodes": depth, "deliberate_mismatches": checks,
               "oracle_sha256_before_after_embedded": oracle_hash, "cli_sha256": digest(args.binary),
               "oracle_report_sha256": digest(oracle_path), "rust_report_sha256": digest(args.output_dir / "rust-valid.json"),
               "runtime_ready": False, "complete_source_ancestry_accepted": False, "gameplay_accepted": False})
    print(f"{len(expected)} exact authored binding comparisons; {len(checks)} altered reports rejected")


if __name__ == "__main__":
    main()
