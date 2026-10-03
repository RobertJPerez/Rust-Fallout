"""Capture the isolated native skin oracle without shell output transcoding."""
import argparse
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("local/nif-skin-oracle-build/Release/nif-skin-oracle.exe"))
    args = parser.parse_args()
    with args.output.open("xb") as output:
        result = subprocess.run([str(args.binary.resolve()), str(args.input.resolve())], stdout=output, check=False)
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
