"""Check the prepared team's paths, task graph and preserved development base."""

from __future__ import annotations

import fnmatch
import json
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]
DOC = ROOT / "docs" / "agents" / "team-v3"


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def git(*args: str, cwd: Path = ROOT) -> str:
    return subprocess.check_output(["git", *args], cwd=cwd, text=True).strip()


def main() -> None:
    ownership = read(DOC / "ownership.json")
    queues = read(DOC / "backlog.json")["tasks"]
    preserved = read(DOC / "preserved-work.json")
    roles = ownership["roles"]
    control = read(ROOT / "local/team/control.json")
    assert control["generation"] == ownership["generation"]
    assert control["mode"] == "paused" and control["stop_requested"] is True
    assert control["run_id"] is None
    assert Path(control["active_coordination_directory"]).resolve() == (ROOT / "local/team-v3").resolve()
    assert not list((ROOT / "local/team-v3/leases").glob("*.json")), "Unexpected active v3 lease"
    assert len(roles) == 8 and set(roles) == set(queues)
    task_list = [task for queue in queues.values() for task in queue]
    tasks = {task["id"]: task for task in task_list}
    assert len(tasks) == len(task_list), "Duplicate task IDs"
    for lane, queue in queues.items():
        assert sum(task["state"] == "ready" for task in queue) >= 3, lane
        for task in queue:
            assert task["consumer"] and task["acceptance"], task["id"]
            assert task["state"] in ("ready", "blocked")
            assert (task["state"] == "ready") == (not task["depends_on"])
            assert all(dep in tasks for dep in task["depends_on"])
            if task["fallback_task_id"]:
                assert task["fallback_task_id"] in {item["id"] for item in queue}

    visited: set[str] = set()
    pending: set[str] = set()

    def visit(ident: str) -> None:
        assert ident not in pending, f"Task dependency cycle at {ident}"
        if ident in visited:
            return
        pending.add(ident)
        for dependency in tasks[ident]["depends_on"]:
            visit(dependency)
        pending.remove(ident)
        visited.add(ident)

    for ident in tasks:
        visit(ident)

    files = git("ls-tree", "-r", "--name-only", preserved["development_base"]).splitlines()
    # Also check reserved module namespaces even before their first source file exists.
    files += ["crates/fallout-runtime/src/reference_state/mod.rs",
              "crates/fallout-runtime/src/physics/mod.rs",
              "crates/fallout-runtime/src/navigation/mod.rs",
              "crates/fallout-runtime/src/actor_rules/mod.rs",
              "crates/fallout-data/src/navigation/mod.rs"]
    for filename in files:
        owners = [lane for lane, role in roles.items()
                  if any(fnmatch.fnmatchcase(filename, pattern) for pattern in role["owned_paths"])
                  and not any(fnmatch.fnmatchcase(filename, pattern)
                              for pattern in role.get("excluded_paths", []))]
        assert len(owners) <= 1, f"Overlapping owners for {filename}: {owners}"

    worktrees: set[Path] = set()
    targets: set[Path] = set()
    for lane, role in roles.items():
        path = Path(role["worktree"])
        assert path.exists() and path not in worktrees, str(path)
        worktrees.add(path)
        assignment = read(ROOT / "local/team-v3" / f"{lane}.assignment.json")
        assert assignment["generation"] == ownership["generation"]
        assert assignment["worktree"] == role["worktree"]
        assert assignment["owned_paths"] == role["owned_paths"]
        assert assignment["ready_queue"] == queues[lane]
        assert assignment["state"] == "paused" and not assignment["implementation_authorized"]
        target = Path(assignment["cargo_target_directory"])
        assert target not in targets
        targets.add(target)
        assert git("branch", "--show-current", cwd=path) == role["branch"]
        if lane != "coordinator":
            assert git("rev-parse", "HEAD", cwd=path) == preserved["development_base"]
            assert not git("status", "--porcelain", cwd=path), str(path)
        prompt = DOC / f"{role['number']}-{lane}.txt"
        assert prompt.exists() and str(path) in prompt.read_text(encoding="utf-8")
    contracts = read(DOC / "contracts.json")
    for contract in contracts["contracts"]:
        assert contract["producer"] in roles
        assert all(lane in roles for lane in contract["consumers"])
    manifest = read(Path(preserved["backup_manifest"]))
    for record in manifest["worktrees"]:
        assert git("rev-parse", record["preservation_ref"]) == record["HEAD"]
        assert Path(record["worktree"]).exists(), "A preserved worktree is missing"
    print(f"Validated {len(roles)} lanes, {len(tasks)} tasks, {len(files)} ownership paths, "
          f"{len(contracts['contracts'])} contracts and seven clean shared-base worktrees.")


if __name__ == "__main__":
    main()
