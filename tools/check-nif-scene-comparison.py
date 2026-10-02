"""Prove the scene comparator detects changed floats, topology, transforms and input identity."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import struct

spec = importlib.util.spec_from_file_location("comparison", Path(__file__).with_name("compare-nif-scenes.py"))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenes", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    probes = comparison.read(args.scenes / "manifest.json")
    oracle = comparison.read(args.oracle)
    target = next(row for row in oracle if row["raw_scene"]["meshes"] and row["raw_scene"]["meshes"][0]["triangles"]
                  and any(m['data']['kind'] == 'material' for m in row['raw_scene']['materials'])
                  and any(m['data']['kind'] == 'texture_set' and any(m['data']['textures']) for m in row['raw_scene']['materials']))
    probe = next(row for row in probes["results"] if row["file"] == target["file"])

    def run(expected, row=probe):
        return comparison.compare({"results": [row]}, [expected], args.scenes, args.cache)

    baseline = run(target)
    if not baseline["all_equal"]:
        raise ValueError("The unmodified comparison must pass before negative checks")
    results = []
    for mutation in ["vertex_float_bit", "triangle_winding", "composed_translation", "source_digest", "material_alpha", "texture_path", "missing_material_projection"]:
        changed, changed_probe = copy.deepcopy(target), copy.deepcopy(probe)
        mesh = changed["raw_scene"]["meshes"][0]
        if mutation == "vertex_float_bit":
            value = mesh["vertices"][0][0]
            bits = struct.unpack("<I", struct.pack("<f", value))[0]
            mesh["vertices"][0][0] = struct.unpack("<f", struct.pack("<I", bits ^ 1))[0]
        elif mutation == "triangle_winding":
            triangle = next(t for t in mesh["triangles"] if len(set(t)) == 3)
            triangle[1], triangle[2] = triangle[2], triangle[1]
        elif mutation == "composed_translation":
            changed["raw_scene"]["world_transforms"][0]["matrix"][0][3] += 1_000_000
        elif mutation == "source_digest":
            changed_probe["sha256"] = "0" * 64
        elif mutation == "material_alpha":
            material = next(m['data'] for m in changed['raw_scene']['materials'] if m['data']['kind'] == 'material')
            material['alpha'] += 0.5
        elif mutation == "texture_path":
            material = next(m['data'] for m in changed['raw_scene']['materials'] if m['data']['kind'] == 'texture_set' and any(m['data']['textures']))
            next(t for t in material['textures'] if t)[0] ^= 1
        else:
            del changed['raw_scene']['materials']
        report = run(changed, changed_probe)
        results.append({"mutation": mutation, "rejected": not report["all_equal"], "difference": report["results"][0].get("difference")})
    output = {"schema_version": 1, "unmodified_equal": True, "source_file": target["file"],
              "original_files_modified": False, "results": results, "all_rejected": all(r["rejected"] for r in results)}
    with args.output.open("x", encoding="utf-8") as file:
        json.dump(output, file, indent=2)
        file.write("\n")
    print(f"{len(results)} deliberate mismatches; all rejected: {output['all_rejected']}")
    return 0 if output["all_rejected"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
