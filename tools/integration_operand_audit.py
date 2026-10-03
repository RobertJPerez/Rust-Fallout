"""Audit operand evidence and require each altered case's specific diagnostic."""
import argparse
import json
from pathlib import Path

import event_operand_comparison as operands


INPUTS = ("probe", "frames", "loaded-scripts", "snapshot", "bindings-rust",
          "bindings-native", "bindings-bundle")
DIAGNOSTICS = (
    ("saved_local_bits", "Saved storage association differs at event "),
    ("unresolved_status", "Saved storage association differs at event "),
    ("dropped_selected_use", "Selected native tuple coverage/order differs at event "),
    ("selected_window", "Selected event window differs"),
    ("full_binding_digest", "Full binder identity differs"),
    ("loaded_source_cohort", "Loaded source cohort differs"),
    ("native_tuple_digest", "Independent native tuple length or digest differs"),
    ("hidden_source_finding", "Strict source finding differs"),
    ("correlated_loaded_and_probe_declaration", "Loaded local declaration differs from native tuple"),
    ("loaded_scrv_type_with_unchanged_reference_kind", "Loaded local declaration differs from native tuple"),
)


def checked_negatives(inputs):
    original = operands.compare
    diagnostics = []

    def compare(*values):
        try:
            return original(*values)
        except RuntimeError as error:
            operands.require(len(diagnostics) < len(DIAGNOSTICS), "Unexpected extra negative case")
            name, expected = DIAGNOSTICS[len(diagnostics)]
            # Do not let the owner's catch-all RuntimeError turn unrelated
            # failures into passing negative evidence.
            if not str(error).startswith(expected):
                raise AssertionError(f"{name} failed for an unrelated reason: {error}") from error
            diagnostics.append({"name": name, "diagnostic": str(error),
                                "expected_diagnostic_prefix": expected})
            raise

    operands.compare = compare
    try:
        rejected = operands.negative_checks(inputs)
    finally:
        operands.compare = original
    operands.require(rejected == [name for name, _ in DIAGNOSTICS], "Negative coverage/order differs")
    operands.require(len(diagnostics) == len(DIAGNOSTICS), "A negative diagnostic was omitted")
    return diagnostics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in (*INPUTS, "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    operands.require(not args.output.exists(), "Use a new audit output")
    paths = {name: getattr(args, name.replace("-", "_")) for name in INPUTS}
    before = {name: operands.sha(operands.read(path, 512 * 1024 * 1024))
              for name, path in paths.items()}
    state, snapshot_sha = operands.snapshot(operands.read(args.snapshot, 16 * 1024 * 1024))
    inputs = [operands.document(args.probe), operands.document(args.frames),
              operands.document(args.loaded_scripts), state,
              operands.document(args.bindings_rust), operands.document(args.bindings_native),
              before["bindings-bundle"], snapshot_sha]
    result = operands.compare(*inputs)
    result["altered_evidence_diagnostics"] = checked_negatives(inputs)
    result["altered_evidence_rejections"] = [row["name"] for row in result["altered_evidence_diagnostics"]]
    after = {name: operands.sha(operands.read(path, 512 * 1024 * 1024))
             for name, path in paths.items()}
    operands.require(before == after, "Operand audit input changed during verification")
    result["inputs"] = before
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(f"Checked {result['selected_native_tuples_checked']} native tuples and ten exact rejection diagnostics")


if __name__ == "__main__":
    main()
