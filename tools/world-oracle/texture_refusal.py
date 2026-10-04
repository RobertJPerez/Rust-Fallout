"""Verify one exact archive-bound texture refusal through existing CLI consumers.

Case JSON stays local: archive_name, archive_sha256, member, block, slot,
raw_path_hex. This tool neither parses retail formats nor repairs source paths.
Run under the lane's heavy build wrapper during an active team run.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def read_json(path, maximum):
    with path.open("rb") as source:
        raw = source.read(maximum + 1)
    if len(raw) > maximum:
        raise ValueError(f"JSON budget exceeded: {path}")
    return json.loads(raw)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def invoke(cli, output, name, arguments, expected_exit):
    report = output / f"{name}.json"
    with (output / f"{name}.log").open("xb") as log:
        result = subprocess.run(
            [str(cli), "--output", str(report), *arguments],
            stdout=log, stderr=subprocess.STDOUT, check=False,
        )
    require(result.returncode == expected_exit, f"{name}: unexpected exit {result.returncode}")
    return read_json(report, 16 * 1024 * 1024)


def verify(cli, install, case_path, output):
    cli, install, case_path = cli.resolve(strict=True), install.resolve(strict=True), case_path.resolve(strict=True)
    output = output.resolve()
    require(not output.is_relative_to(install), "evidence must be outside the installation")
    case = read_json(case_path, 65536)
    archive_name = case["archive_name"]
    require(Path(archive_name).name == archive_name and "/" not in archive_name and "\\" not in archive_name,
            "case must name one top-level archive")
    archive = install / "Data" / archive_name
    expected_raw = bytes.fromhex(case["raw_path_hex"])
    require(0 < len(expected_raw) <= 4096, "expected texture path budget")
    require(len(case["member"].encode("utf-8")) <= 4096, "member path budget")
    archive_sha = digest(archive)
    require(archive_sha == case["archive_sha256"], "selected source archive changed")
    cli_sha, case_sha = digest(cli), digest(case_path)
    output.mkdir(parents=True, exist_ok=False)
    model_cache, texture_cache = output / "model-cache", output / "texture-cache"
    model_cache.mkdir()
    texture_cache.mkdir()
    asset = invoke(cli, output, "model", ["asset", str(archive), case["member"],
                   "--cache-root", str(model_cache), "--inspect-nif"], 0)
    manifest = asset["cache"]["manifest"]
    require(manifest["identity"]["source_sha256"] == archive_sha,
            "model cache has another source archive")
    require(manifest["identity"]["profile"] == "nv-original", "model cache profile mismatch")
    require(manifest["identity"]["transform_version"] == "nv-bsa104-decode-v1", "unexpected importer")
    require(manifest["sha256"] == asset["sha256"] and manifest["bytes"] == asset["decoded_bytes"],
            "model cache extent/digest mismatch")
    model = model_cache / f"{asset['cache']['key']}.blob"
    require(model.stat().st_size == manifest["bytes"] and digest(model) == manifest["sha256"],
            "model artifact differs from source receipt")
    report = invoke(cli, output, "textures", ["nif-assets", str(model), "--install", str(install),
                    "--texture-cache", str(texture_cache)], 1)
    require(report["runtime_ready"] is False, "source inspection admitted runtime readiness")
    require(len(report["models"]) == 1 and report["models"][0]["sha256"] == asset["sha256"],
            "texture consumer read another model")
    selected = [texture for texture in report["models"][0]["textures"]
                if texture["block"] == case["block"] and texture["slot"] == case["slot"]]
    require(len(selected) == 1, "selected source block/slot is missing or ambiguous")
    texture = selected[0]
    require(bytes(texture["raw_path"]) == expected_raw, "authored texture path changed")
    require(texture["asset_path"] is None and texture["error"], "unsafe source path was admitted")
    fingerprints = {str(archive): archive_sha}
    for item in report["textures"]:
        if item["cache"] is not None:
            require(len(item["candidates"]) == 1, "cached texture has ambiguous source")
            identity = item["cache"]["manifest"]["identity"]
            source = item["candidates"][0]["container"]
            source_sha = identity["source_sha256"]
            require(source not in fingerprints or fingerprints[source] == source_sha,
                    "inconsistent source archive fingerprints")
            fingerprints[source] = source_sha
    for path, sha in fingerprints.items():
        require(digest(Path(path)) == sha, f"source archive changed: {path}")
    require(digest(model) == asset["sha256"] and digest(cli) == cli_sha and digest(case_path) == case_sha,
            "model, executable or case changed during inspection")
    receipt = {
        "schema_version": 1, "cli_sha256": cli_sha, "case_sha256": case_sha,
        "source_fingerprints": fingerprints, "model_sha256": asset["sha256"],
        "model_decoded_bytes": asset["decoded_bytes"], "block": case["block"], "slot": case["slot"],
        "raw_path_sha256": hashlib.sha256(expected_raw).hexdigest(), "reason": texture["error"],
        "precise_refusal_verified": True, "texture_failures": report["failures"],
        "runtime_ready": False, "retail_handling_verified": False, "gameplay_accepted": False,
        "scope": "Existing archive importer/cache -> exact model -> existing authored texture path refusal",
    }
    with (output / "receipt.json").open("x", encoding="utf-8") as destination:
        json.dump(receipt, destination, indent=2)
        destination.write("\n")
    print(f"Exact source refusal verified; model {asset['sha256']}, block {case['block']} slot {case['slot']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("cli", "install", "case", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        verify(arguments.cli, arguments.install, arguments.case, arguments.output)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"Texture refusal proof incomplete: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
