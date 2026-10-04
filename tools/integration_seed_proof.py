"""Copy the exact private inputs a fresh integration proof expects.

The source is an earlier coordinator worktree with verified receipts. This never
copies game files from the installation or replaces binaries built in the new
worktree. Every copied byte is checked against its existing seed/report hash.
"""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil


REBUILT = {
    "local/actor-oracle-build/Release/actor-oracle.exe",
    "local/nif-skin-oracle-build/Release/nif-skin-oracle.exe",
    "local/operand-oracle-build/Release/operand-oracle.exe",
}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def confined(root: Path, name: str) -> Path:
    relative = Path(name)
    if relative.is_absolute() or ".." in relative.parts or relative.parts[0] != "local":
        raise ValueError(f"proof input is not a local relative path: {name}")
    path = root / relative
    if not path.resolve(strict=False).is_relative_to(root):
        raise ValueError(f"proof input escapes worktree: {name}")
    return path


def copy_verified(source: Path, target: Path, name: str, expected: str) -> dict:
    original = confined(source, name)
    destination = confined(target, name)
    if digest(original) != expected:
        raise ValueError(f"preserved input differs from its receipt: {name}")
    if destination.exists():
        if digest(destination) != expected:
            raise ValueError(f"existing proof input differs; refusing overwrite: {name}")
        action = "already_present"
    else:
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.with_name(f"{destination.name}.seed-{os.getpid()}.tmp")
        with original.open("rb") as incoming, temporary.open("xb") as outgoing:
            shutil.copyfileobj(incoming, outgoing, 1024 * 1024)
            outgoing.flush()
            os.fsync(outgoing.fileno())
        if digest(temporary) != expected:
            raise ValueError(f"copied proof input differs: {name}")
        # On Windows rename refuses an existing destination. A concurrent proof
        # setup cannot replace another writer's file.
        temporary.rename(destination)
        action = "copied"
    return {"path": name, "sha256": expected, "bytes": destination.stat().st_size,
            "action": action}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--receipt", default="local/integration-seed-setup.json")
    args = parser.parse_args()
    source, target = args.source.resolve(), args.destination.resolve()
    if source == target:
        raise ValueError("proof source and destination must differ")
    receipt = confined(target, args.receipt)
    if receipt.exists():
        raise ValueError(f"setup receipt already exists: {receipt}")

    module = target / "tools/integration_proof.py"
    spec = importlib.util.spec_from_file_location("proof_inputs", module)
    if spec is None or spec.loader is None:
        raise ValueError("integration proof module is missing")
    proof = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(proof)

    seed_name = "local/integration-seed.json"
    seed = json.loads(confined(source, seed_name).read_bytes())
    rows = [copy_verified(source, target, seed_name, digest(confined(source, seed_name)))]
    for entry in seed["files"]:
        name = entry["path"]
        if name in REBUILT:
            rebuilt = confined(target, name)
            if not rebuilt.is_file():
                raise ValueError(f"freshly built proof binary is missing: {name}")
            continue
        rows.append(copy_verified(source, target, name, entry["sha256"]))

    for name, report_name, field in proof.HISTORICAL:
        report = json.loads((target / report_name).read_bytes())
        rows.append(copy_verified(source, target, name, report[field]))

    manifest_name = "local/integration-actor-fixtures/manifest.json"
    manifest_path = confined(source, manifest_name)
    manifest = json.loads(manifest_path.read_bytes())
    rows.append(copy_verified(source, target, manifest_name, digest(manifest_path)))
    for entry in manifest["files"]:
        rows.append(copy_verified(source, target, entry["path"], entry["sha256"]))

    # Run the proof's own readers after setup. This catches missing inputs and
    # protects rebuilt binaries without making this helper a second verifier.
    proof.frozen_inputs(target)
    proof.frozen_binaries(target)
    proof.frozen_research(target)
    receipt.parent.mkdir(parents=True, exist_ok=True)
    with receipt.open("x", encoding="utf-8", newline="\n") as output:
        json.dump({"schema_version": 1, "seed_sha256": digest(confined(target, seed_name)),
                   "files": rows, "rebuilt_binaries": sorted(REBUILT),
                   "proof_preflight_passed": True}, output, indent=2)
        output.write("\n")
    print(f"Verified {len(rows)} isolated proof inputs: {receipt}")


if __name__ == "__main__":
    main()
