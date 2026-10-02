"""Compare a cell's Rust NIF inventories with the independent nifly executable."""
import argparse
import hashlib
import json
from pathlib import Path


def compare(cell, oracle, cache_root):
    by_file = {entry["file"]: entry for entry in oracle}
    if len(by_file) != len(oracle):
        raise ValueError("Duplicate oracle filename")
    results = []
    for model in cell["model_probes"]:
        if model["error"] or not model["cache"] or not model["nif"]:
            results.append({"path_bytes": model["path"], "equal": False, "error": "Rust probe failed or has no cached oracle input"})
            continue
        filename = model["cache"]["key"] + ".blob"
        expected = by_file.pop(filename, None)
        actual = model["nif"]
        payload = (cache_root / filename).read_bytes()
        digest_equal = hashlib.sha256(payload).hexdigest() == model["sha256"] and len(payload) == model["decoded_bytes"]
        actual_blocks = [{"type": actual["block_types"][block["type_index"]], "type_index": block["type_index"], "bytes": block["bytes"]}
                         for block in actual["blocks"]]
        equal = (expected is not None and expected["load_code"] == 0 and digest_equal
                 and all(actual[key] == expected[key] for key in ("version", "user_version", "bethesda_version", "roots"))
                 and actual_blocks == expected["blocks"])
        results.append({"path_bytes": model["path"], "sha256": model["sha256"], "decoded_bytes": len(payload),
                        "blocks_compared": len(actual_blocks), "cache_digest_matches": digest_equal, "equal": equal})
    return {"schema_version": 1, "profile": "nv-original", "cell": cell["key"],
            "oracle": "nifly cca0a770094bb962fb28ea1fec5ea903e68fda8e",
            "comparison": "Exact version tuple, block type/index/size sequence, footer roots; cache bytes rehashed",
            "files_compared": len(results), "blocks_compared": sum(r.get("blocks_compared", 0) for r in results),
            "all_equal": bool(results) and all(r["equal"] for r in results) and not by_file,
            "unused_oracle_files": sorted(by_file), "results": results,
            "gameplay_or_rendering_accepted": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cell", required=True, type=Path)
    parser.add_argument("--oracle", required=True, type=Path)
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    read = lambda path: json.loads(path.read_text(encoding="utf-8-sig"))
    report = compare(read(args.cell), read(args.oracle), args.cache)
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output, indent=2)
        output.write("\n")
    print(f"{report['files_compared']} NIFs; {report['blocks_compared']} blocks; all equal: {report['all_equal']}")
    raise SystemExit(0 if report["all_equal"] else 1)
