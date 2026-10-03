"""Capture the isolated native skin oracle without shell output transcoding."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("local/nif-skin-oracle-build/Release/nif-skin-oracle.exe"))
    parser.add_argument("--include-partitions", action="store_true")
    parser.add_argument("--include-bindings", action="store_true")
    args = parser.parse_args()
    digest = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    command = [str(args.binary.resolve()), str(args.input.resolve())]
    if args.include_bindings:
        command.append("--include-bindings")
    elif args.include_partitions:
        command.append("--include-partitions")
    with args.output.open("xb") as output:
        result = subprocess.run(command, stdout=output, check=False)
    after = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    document = json.loads(args.output.read_text(encoding="utf-8"))
    if digest != after or document.get("oracle_binary_sha256") != digest:
        raise RuntimeError("oracle binary changed or embedded digest differs; retained output")
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
