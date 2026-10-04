"""Check one engineering palette against exact rational native source fields.

The native report is already independently decoded/compared source evidence.
This checker does not parse another NIF, normalize weights or certify playback.
"""
from __future__ import annotations

import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_json(path, limit=128 * 1024 * 1024):
    if path.stat().st_size > limit:
        raise ValueError("source report exceeds byte admission")
    return json.loads(path.read_text(encoding="utf-8-sig"))


def number(bits):
    value = struct.unpack("<f", struct.pack("<I", bits))[0]
    return Fraction(value)


def identity():
    return [[Fraction(int(r == c)) for c in range(4)] for r in range(4)]


def matrix(transform):
    result = identity()
    for r in range(3):
        for c in range(3):
            result[r][c] = number(transform["rotation_bits"][r][c]) * number(transform["scale_bits"])
        result[r][3] = number(transform["translation_bits"][r])
    return result


def multiply(a, b):
    # Full 4x4 rational multiplication, unlike the implementation's affine rows.
    return [[sum((a[r][k] * b[k][c] for k in range(4)), Fraction())
             for c in range(4)] for r in range(4)]


def absolute(a):
    return [[abs(value) for value in row] for row in a]


def references(row, geometry):
    owner = next(o for o in row["owners"] if o["geometry"] == geometry)
    blocks = {b["block"]: b["data"] for b in row["skins"]}
    instance = blocks[owner["instance"]]["instance"]
    data = blocks[instance["data"]]
    nodes = {n["block"]: n for n in row["bindings"]["nodes"]}
    if len(nodes) > 100_000 or len(instance["bones"]) > 100_000:
        raise ValueError("source count admission exceeded")
    skin = matrix(data["transform"])
    expected = []
    sums = [Fraction() for _ in range(owner["vertex_count"])]
    for ordinal, bone in enumerate(data["bones"]):
        node = instance["bones"][ordinal]
        cursor = node
        chain = []
        while cursor != instance["skeleton_root"]:
            if cursor is None or cursor not in nodes or len(chain) >= 1024:
                raise ValueError("source ancestry unresolved or beyond admission")
            chain.append(matrix(nodes[cursor]["transform"]))
            cursor = nodes[cursor]["parent"]
        # Reference folds from the root to the bone; Rust walks upwards and
        # prepends locals. The order differs while the exact algebra agrees.
        factors = [skin, *reversed(chain), matrix(bone["transform"])]
        exact, condition = identity(), identity()
        for factor in factors:
            exact = multiply(exact, factor)
            condition = multiply(condition, absolute(factor))
        expected.append((ordinal, node, exact, condition, len(factors)))
        for weight in bone["weights"]:
            sums[weight["vertex"]] += number(weight["weight_bits"])
    return owner, instance, expected, sums


def compare_palette(evaluation, row, geometry):
    owner, instance, expected, sums = references(row, geometry)
    if evaluation["source_sha256"] != row["sha256"]:
        raise ValueError("whole source identity differs")
    if (evaluation["geometry"], evaluation["geometry_data"], evaluation["instance"],
        evaluation["skin_data"], evaluation["skeleton_root"]) != (
        geometry, owner["geometry_data"], owner["instance"], instance["data"], instance["skeleton_root"]):
        raise ValueError("selected source block identity differs")
    if len(evaluation["positions"]) != owner["vertex_count"] or len(evaluation["palette"]) != len(expected):
        raise ValueError("evaluated vertex or palette cardinality differs")
    if evaluation["weight_sums"] != [float(value) for value in sums]:
        raise ValueError("raw independently summed weights differ")
    if evaluation["retail_behavior_verified"] is not False:
        raise ValueError("engineering receipt claims retail verification")
    maximum_error = Fraction()
    for actual, (ordinal, node, exact, condition, length) in zip(evaluation["palette"], expected):
        if (actual["ordinal"], actual["node"]) != (ordinal, node):
            raise ValueError("source palette order differs")
        # Each affine dot uses <=7 rounded operations. Binary32 rotation*scale
        # promotion is exact in binary64. gamma_(7*m) times the absolute product
        # bounds an m-factor chain (ordinary finite arithmetic in this input).
        count = 7 * length
        unit_roundoff = Fraction(1, 2**53)
        gamma = count * unit_roundoff / (1 - count * unit_roundoff)
        for r in range(3):
            for c in range(4):
                error = abs(Fraction(actual["matrix"][r][c]) - exact[r][c])
                bound = gamma * condition[r][c] + Fraction(1, 2**1074)
                if error > bound:
                    raise ValueError(f"palette numeric mismatch at bone {ordinal} [{r},{c}]")
                maximum_error = max(maximum_error, error)
    return dict(palette_entries=len(expected), coefficients=len(expected) * 12,
                vertices=owner["vertex_count"], maximum_absolute_palette_error=float(maximum_error),
                numerical_bound="gamma_(7*m) * absolute matrix product + 2^-1074")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--native-report", required=True, type=Path)
    parser.add_argument("--geometry", required=True, type=int)
    parser.add_argument("--tolerance", required=True, type=float)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()
    if args.input.stat().st_size > 64 * 1024 * 1024:
        raise ValueError("source input exceeds byte admission")
    args.output_dir.mkdir(exist_ok=False)
    binary_hash, source_hash, report_hash = digest(args.binary), digest(args.input), digest(args.native_report)
    native = read_json(args.native_report)
    if native["schema_version"] != 3 or native["graph_membership_checked"] is not True:
        raise ValueError("native source binding contract missing")
    row = next(row for row in native["files"] if row["file"] == args.input.name)
    if row["sha256"] != source_hash or row["decoded_bytes"] != args.input.stat().st_size:
        raise ValueError("native source digest/length mismatch")
    invocations = []
    for name, geometry, tolerance, expected_exit, expected_error in (
        ("positive", args.geometry, args.tolerance, 0, None),
        ("strict-zero-tolerance", args.geometry, 0.0, 1, "raw weight sum"),
        ("unowned-geometry", 2**32-1, args.tolerance, 1, "no decoded skin owner"),
    ):
        output = args.output_dir / (name + ".json")
        command = [str(args.binary.resolve()), "nif-skin", str(args.input.resolve()),
                   "--pose-geometry", str(geometry), "--pose-weight-tolerance", str(tolerance),
                   "--output", str(output.resolve())]
        process = subprocess.run(command, capture_output=True, text=True, check=False)
        (args.output_dir / (name + ".stdout.txt")).write_text(process.stdout)
        (args.output_dir / (name + ".stderr.txt")).write_text(process.stderr)
        if process.returncode != expected_exit:
            raise ValueError(f"{name}: unexpected CLI exit {process.returncode}: {process.stderr}")
        receipt = read_json(output)
        if expected_error:
            if receipt["failures"] != 1 or expected_error not in receipt["error"] or receipt["evaluation"] is not None:
                raise ValueError(f"{name}: intended refusal receipt missing")
        else:
            if receipt["failures"] != 0 or receipt["contract"] != "engineering-source-local-skin-v1":
                raise ValueError("positive engineering receipt missing")
            numeric = compare_palette(receipt["evaluation"], row, args.geometry)
            altered = json.loads(json.dumps(receipt["evaluation"]))
            altered["palette"][0]["matrix"][0][3] += 0.125
            try:
                compare_palette(altered, row, args.geometry)
            except ValueError as error:
                if "palette numeric mismatch" not in str(error):
                    raise
            else:
                raise ValueError("altered palette was accepted")
        invocations.append(dict(command=command, exit_code=process.returncode, receipt_sha256=digest(output)))
    if (digest(args.binary), digest(args.input), digest(args.native_report)) != (binary_hash, source_hash, report_hash):
        raise ValueError("frozen binary/source/reference changed")
    summary = dict(contract="engineering-source-local-skin-v1", binary_sha256=binary_hash,
                   source_sha256=source_hash, native_report_sha256=report_hash, numeric=numeric,
                   invocations=invocations, intended_refusals=2, altered_palette_rejections=1,
                   retail_behavior_verified=False)
    (args.output_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
