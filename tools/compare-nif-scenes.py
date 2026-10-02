"""Compare Rust scene payloads with raw nifly factory output on identical local bytes."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def first_difference(actual, expected, path=""):
    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            return path + ": expected object"
        for key, value in expected.items():
            if key not in actual:
                return path + "." + key + ": missing"
            difference = first_difference(actual[key], value, path + "." + key)
            if difference:
                return difference
    elif isinstance(expected, list):
        if not isinstance(actual, list) or len(actual) != len(expected):
            return path + ": array lengths differ"
        for index, (a, e) in enumerate(zip(actual, expected)):
            difference = first_difference(a, e, f"{path}[{index}]")
            if difference:
                return difference
    elif isinstance(actual, float) or isinstance(expected, float):
        if not isinstance(actual, (float, int)) or not isinstance(expected, (float, int)):
            return path + ": numeric type differs"
        if not math.isfinite(actual) or not math.isfinite(expected) or struct.pack("<f", actual) != struct.pack("<f", expected):
            return f"{path}: f32 bits differ ({actual} vs {expected})"
    elif type(actual) is not type(expected) or actual != expected:
        return f"{path}: values differ ({actual} vs {expected})"
    return None


def compare(probes, oracle, directory, cache):
    expected_files = {entry["file"]: entry for entry in oracle}
    if len(expected_files) != len(oracle):
        raise ValueError("duplicate oracle filename")
    seen = set()
    rows = []
    for probe in probes["results"]:
        row = {"file": probe.get("file"), "path_bytes": probe["path_bytes"], "sha256": probe["sha256"], "equal": False}
        rows.append(row)
        name = probe.get("file")
        if not name or name in seen or Path(name).name != name:
            row["difference"] = "missing, duplicate or invalid input filename"
            continue
        seen.add(name)
        expected = expected_files.pop(name, None)
        if "error" in probe or not expected or expected.get("load_code") != 0 or "raw_scene" not in expected:
            row["difference"] = "missing or failed parser/oracle output"
            continue
        document = read(directory / probe["report"])
        payload = (cache / name).read_bytes()
        if hashlib.sha256(payload).hexdigest() != probe["sha256"] or document["sha256"] != probe["sha256"] or len(payload) != document["decoded_bytes"]:
            row["difference"] = "input hash or length differs"
            continue
        actual, reference = document["scene"], expected["raw_scene"]
        difference = first_difference(actual["objects"], reference["objects"], "objects")
        # Both sides must carry the new projection; an older oracle cannot certify it.
        if "materials" not in actual or "materials" not in reference:
            difference = difference or "material projection missing"
        else:
            difference = difference or first_difference(actual["materials"], reference["materials"], "materials")
        actual_meshes = []
        for mesh in actual["meshes"]:
            projected = dict(mesh)
            topology = mesh["topology"]
            if topology["kind"] == "strips":
                projected.update(strip_lengths=topology["lengths"], has_points=topology["present"], strip_indices=topology["indices"])
            else:
                projected["match_groups"] = topology["match_groups"]
            actual_meshes.append(projected)
        difference = difference or first_difference(actual_meshes, reference["meshes"], "meshes")
        max_absolute_error = 0.
        if len(actual["world_transforms"]) != len(reference["world_transforms"]):
            difference = difference or "world transform count differs"
        else:
            for a, e in zip(actual["world_transforms"], reference["world_transforms"]):
                if (a["block"], a["parent"]) != (e["block"], e["parent"]):
                    difference = difference or "world transform block/parent differs"
                for ar, er in zip(a["matrix"], e["matrix"]):
                    for av, ev in zip(ar, er):
                        max_absolute_error = max(max_absolute_error, abs(av - ev))
                        if not math.isfinite(av) or not math.isfinite(ev) or not math.isclose(av, ev, abs_tol=1e-4, rel_tol=1e-5):
                            difference = difference or f"composed transform differs at block {a['block']}"
        row.update(equal=difference is None, difference=difference,
                   objects=len(actual["objects"]), meshes=len(actual["meshes"]),
                   materials=len(actual.get("materials", [])),
                   vertices=sum(len(m["vertices"]) for m in actual["meshes"]),
                   triangles=sum(len(m["triangles"]) for m in actual["meshes"]),
                   max_world_transform_absolute_error=max_absolute_error)
    return {"schema_version": 1, "oracle": "raw nifly factories cca0a770094bb962fb28ea1fec5ea903e68fda8e; no PrepareData",
            "scope": "supported object and material fields, raw texture bytes/slots, vertex attributes, topology, triangles, composed transforms",
            "tolerance": "source floats: identical f32 bits; topology and references: exact; composed f64 vs nifly f32: abs 1e-4 or rel 1e-5",
            "files": len(rows), "all_equal": bool(rows) and all(r["equal"] for r in rows) and not expected_files,
            "unused_oracle_files": sorted(expected_files), "results": rows, "rendering_or_gameplay_accepted": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenes", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = compare(read(args.scenes / "manifest.json"), read(args.oracle), args.scenes, args.cache)
    report["scene_manifest_sha256"] = hashlib.sha256((args.scenes / "manifest.json").read_bytes()).hexdigest()
    report["oracle_report_sha256"] = hashlib.sha256(args.oracle.read_bytes()).hexdigest()
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output, indent=2)
        output.write("\n")
    print(f"{report['files']} scenes; all equal: {report['all_equal']}")
    for row in report["results"]:
        if not row["equal"]:
            print(row["file"], row.get("difference"))
    raise SystemExit(0 if report["all_equal"] else 1)
