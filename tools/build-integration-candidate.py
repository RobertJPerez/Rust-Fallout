"""Build and check a committed candidate privately before a proof freeze."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

from integration_proof import digest, git, new_local_path, require, snapshot, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path("."))
    parser.add_argument("--run-directory", type=Path, required=True)
    args = parser.parse_args()
    root = args.repository.resolve()
    run = new_local_path(root, args.run_directory)
    require(not git(root, "status", "--porcelain"), "Commit candidate before building")
    revision = git(root, "rev-parse", "HEAD").decode().strip()
    source = snapshot(root, revision)
    run.mkdir()
    write_json(run / "source-start.json", source)
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(root / "target")
    commands = []

    def execute(name, arguments):
        arguments = [str(value) for value in arguments]
        print(f"Building/checking {name}", flush=True)
        log = run / f"{name}.log"
        with log.open("xb") as output:
            result = subprocess.run(arguments, cwd=root, env=environment, stdout=output, stderr=subprocess.STDOUT)
        commands.append({"name": name, "arguments": arguments, "exit_code": result.returncode,
                         "log": log.relative_to(root).as_posix(), "log_sha256": digest(log)})
        require(result.returncode == 0, f"{name} failed; see {log}")

    powershell = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
    # Use the same wrappers as the existing worktree workflow and private targets.
    execute("workspace-check", [*powershell, root / "tools/check.ps1"])
    execute("source-lane-guards", [sys.executable, root / "tools/test_source_lane_guards.py"])
    execute("integration-guards", [sys.executable, root / "tools/test_integration_guards.py"])
    execute("release-cli", [*powershell, root / "tools/cargo.ps1", "build", "--locked", "--release", "-p", "fallout-cli", "--bins"])
    execute("actor-oracle", [*powershell, root / "tools/actor-oracle/build.ps1", "-BuildDirectory", root / "local/actor-oracle-build"])
    execute("skin-oracle", [*powershell, root / "tools/build-nif-skin-oracle.ps1", "-NiflySource", root / ".research/nifly",
            "-BuildDirectory", root / "local/nif-skin-oracle-build"])
    execute("operand-oracle", [*powershell, root / "tools/build-integration-operand-oracle.ps1", "-BuildDirectory", root / "local/operand-oracle-build"])
    require(git(root, "rev-parse", "HEAD").decode().strip() == revision, "HEAD changed during build")
    require(snapshot(root, revision) == source, "Source changed during build")
    binaries = {name: digest(root / name) for name in (
        "target/release/fallout.exe", "target/release/fallout-evidence.exe",
        "local/actor-oracle-build/Release/actor-oracle.exe",
        "local/actor-oracle-build/Release/actor-body-tests.exe",
        "local/nif-skin-oracle-build/Release/nif-skin-oracle.exe",
        "local/operand-oracle-build/Release/operand-oracle.exe")}
    write_json(run / "source-finish.json", source)
    write_json(run / "build.json", {"schema_version": 1, "revision": revision,
               "source_snapshot_sha256": source["sha256"], "cargo_target_directory": str(root / "target"),
               "commands": commands, "binaries": binaries, "source_unchanged": True,
               "retail_parity_accepted": False})
    print(f"Private candidate build receipt: {run / 'build.json'}")


if __name__ == "__main__":
    main()
