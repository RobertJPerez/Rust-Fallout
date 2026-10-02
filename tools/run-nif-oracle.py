"""Capture the native oracle as UTF-8 bytes without PowerShell redirect transcoding."""
import argparse
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--input", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--mode", choices=["container", "scene", "diagnostics"], default="container")
parser.add_argument("--binary", type=Path, default=Path("local/nif-oracle-build/Release/nif-oracle.exe"))
args = parser.parse_args()
command = [str(args.binary.resolve()), str(args.input)]
if args.mode != "container":
    command.append("--scene" if args.mode == "scene" else "--scene-diagnostics")
with args.output.open("xb") as output:
    result = subprocess.run(command, stdout=output)
raise SystemExit(result.returncode)
