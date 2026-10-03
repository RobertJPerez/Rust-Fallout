"""Capture the isolated animation oracle and check actual binary provenance."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("local/nif-animation-oracle-build/Release/nif-animation-oracle.exe"))
    parser.add_argument("--include-keyframes", action="store_true")
    args = parser.parse_args()
    before = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    with args.output.open("xb") as output:
        command = [str(args.binary.resolve()), str(args.input.resolve())]
        if args.include_keyframes:
            command.append("--include-keyframes")
        result = subprocess.run(command, stdout=output, check=False)
    after = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    document = json.loads(args.output.read_text(encoding="utf-8"))
    if before != after or document.get("oracle_binary_sha256") != before:
        raise RuntimeError("oracle executable changed or embedded digest differs; retained output")
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
