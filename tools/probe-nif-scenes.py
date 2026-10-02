"""Decode every cached model in a cell report; preserve failed inputs in the manifest."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cell", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/release/fallout.exe"))
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    rows = []
    for model in read(args.cell)["model_probes"]:
        row = {"path_bytes": model["path"], "sha256": model["sha256"]}
        if model["error"] or not model["cache"]:
            row["error"] = "Model probe did not produce an oracle input"
        else:
            name = model["cache"]["key"]
            source = args.cache / (name + ".blob")
            output = args.output_dir / (name + ".json")
            row["file"] = source.name
            row["report"] = output.name
            if hashlib.sha256(source.read_bytes()).hexdigest() != model["sha256"]:
                row["error"] = "Cached source digest differs from the cell report"
            else:
                run = subprocess.run([str(args.binary.resolve()), "nif-scene", str(source), "--output", str(output)], capture_output=True, text=True)
                row["exit_code"] = run.returncode
                if run.returncode:
                    row["error"] = run.stderr.strip()
                else:
                    scene = read(output)["scene"]
                    row.update(objects=len(scene["objects"]), meshes=len(scene["meshes"]), materials=len(scene["materials"]),
                               texture_references=len(scene["textures"]), unsafe_texture_paths=sum(t['error'] is not None for t in scene['textures']),
                               vertices=sum(m["vertex_count"] for m in scene["meshes"]),
                               triangles=sum(len(m["triangles"]) for m in scene["meshes"]),
                               triangle_count_mismatches=sum(not m["source_triangle_count_matches"] for m in scene["meshes"]),
                               strip_degenerate_triangles=sum(m["strip_degenerate_triangles"] for m in scene["meshes"]),
                               unsupported_scene_edges=scene["unsupported_scene_edges"])
        rows.append(row)
    report = {"schema_version": 1, "cell_report": str(args.cell), "cell_report_sha256": hashlib.sha256(args.cell.read_bytes()).hexdigest(),
              "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(), "results": rows,
              "files": len(rows), "failures": sum("error" in r for r in rows)}
    (args.output_dir / "manifest.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"{report['files']} inputs, {report['failures']} failures")
    for row in rows:
        if "error" in row:
            print(bytes(row["path_bytes"]).decode("ascii", "backslashreplace"), row["error"])
    return 1 if report["failures"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
