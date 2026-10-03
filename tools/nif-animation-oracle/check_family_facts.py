"""Compare metadata-only Rust class facts to the immutable pinned nifxml."""
import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nifxml", type=Path, default=Path("G:/Rust-Fallout/.research/nifxml"))
    args = parser.parse_args()
    revision = subprocess.check_output(["git", "-C", str(args.nifxml), "rev-parse", "HEAD"], text=True).strip()
    source = (args.nifxml / "nif.xml").read_bytes()
    if revision != "970a6238218a106daaeb89a61bcda0eeaf9d08c4" or hashlib.sha256(source).hexdigest() != "d6b76a83ea5fbadd21da5f06348b236a1c43da6bb76d108a2e8b749c22618dd6":
        raise AssertionError("nifxml revision/content differs from the declared pin")
    classes = {n.get("name"): n for n in ET.fromstring(source) if n.tag == "niobject"}
    families = ["NiTimeController", "NiInterpolator", "NiObjectNET", "NiTextKeyExtraData", "NiControllerManager", "NiTransformData", "BSAnimNotes"]
    expected = {}
    for name in sorted(classes):
        ancestors = set()
        current = name
        while current:
            if current in ancestors:
                raise AssertionError("cyclic source inheritance facts")
            ancestors.add(current)
            current = classes[current].get("inherit") if current in classes else None
        expected[name] = sum(1 << i for i, family in enumerate(families) if family in ancestors)
    path = Path(__file__).resolve().parents[2] / "crates/fallout-data/src/nif_animation/families.rs"
    actual_pairs = [(name, int(mask)) for name, mask in re.findall(r'\("([^"]+)", (\d+)\),', path.read_text())]
    if actual_pairs != sorted(expected.items()) or len(expected) != 555:
        raise AssertionError("Rust class names/order/inheritance masks differ from pinned metadata")
    print("555 exact metadata-only class facts; no payload parser/evaluation support inferred")


if __name__ == "__main__":
    main()
