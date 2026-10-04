"""Run one team-v2 focused build under a shared, crash-safe Windows mutex.

Workers own separate checkouts and Cargo targets, but a few simultaneous Rust
compiles can exhaust this machine's available memory. The kernel releases this
mutex if a worker exits unexpectedly, so the next worker need not wait for the
coordinator to repair a stale reservation.
"""

from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
TEAM = ROOT / "local" / "team-v2"
CONTROL = ROOT / "local" / "team" / "control.json"
MUTEX_NAME = "Local\\RustFalloutTeamV2FocusedCargo"
WAIT_OBJECT_0 = 0
WAIT_ABANDONED = 0x80
WAIT_TIMEOUT = 0x102
READ_ATTEMPTS = 6
READ_RETRY_SECONDS = 0.05


def read_coordination_text(path: Path) -> str:
    # A worker atomically replaces its status on Windows. A reader can briefly
    # lose the open race to that replacement even though the file is healthy.
    # Retry only that filesystem race; persistent absence still stops the build.
    for attempt in range(READ_ATTEMPTS):
        try:
            return path.read_text(encoding="utf-8")
        except (PermissionError, FileNotFoundError):
            if attempt + 1 == READ_ATTEMPTS:
                raise
            time.sleep(READ_RETRY_SECONDS)
    raise AssertionError("unreachable coordination read")


def read_json(path: Path) -> dict:
    value = json.loads(read_coordination_text(path))
    if not isinstance(value, dict):
        raise ValueError(f"Expected a JSON object: {path}")
    return value


def check_authorization(lane: str) -> tuple[dict, dict]:
    control = read_json(CONTROL)
    assignment = read_json(TEAM / f"{lane}.assignment.json")
    lease = read_json(TEAM / "leases" / f"{lane}.json")
    status = read_json(TEAM / f"{lane}.status.json")
    if control.get("mode") != "active" or control.get("stop_requested"):
        raise RuntimeError("Team is stopped or inactive")
    if control.get("focused_build_policy") != "automatic_mutex":
        raise RuntimeError("Automatic focused builds are not active yet; current legacy build must finish")
    if assignment.get("state") != "active" or not assignment.get("implementation_authorized"):
        raise RuntimeError(f"{lane} assignment is inactive")
    if not (control.get("run_id") == assignment.get("run_id") == status.get("run_id")):
        raise RuntimeError(f"{lane} run_id does not match active control")
    # Some active leases predate run_id stamping. Their worker-owned status has
    # the current run and the same session UUID, so they remain unambiguous.
    if lease.get("run_id") not in (None, control["run_id"]):
        raise RuntimeError(f"{lane} lease belongs to another run")
    if not (control.get("generation") == assignment.get("generation") == lease.get("generation")):
        raise RuntimeError(f"{lane} generation does not match active control")
    if lease.get("lane") != lane or assignment.get("lane") != lane:
        raise RuntimeError(f"{lane} lease or assignment belongs to another lane")
    if status.get("session_uuid") != lease.get("session_uuid"):
        raise RuntimeError(f"{lane} status session does not hold the current lease")
    if status.get("state") in ("stopped", "paused", "blocked"):
        raise RuntimeError(f"{lane} is not active")
    if Path(assignment["worktree"]).resolve() != Path.cwd().resolve():
        raise RuntimeError(f"Run from assigned worktree: {assignment['worktree']}")
    if Path(lease["worktree"]).resolve() != Path.cwd().resolve():
        raise RuntimeError("Lease worktree differs from current directory")
    if os.environ.get("CARGO_TARGET_DIR") != assignment.get("cargo_target_directory"):
        raise RuntimeError("Set CARGO_TARGET_DIR to the assigned private target directory")
    # STOP can arrive through an outbox before the control-file update.
    for outbox in TEAM.glob("*.outbox.jsonl"):
        for line in read_coordination_text(outbox).splitlines():
            if not line.strip():
                continue
            row = json.loads(line)
            if row.get("run_id") == control["run_id"] and row.get("type") == "stop_requested":
                raise RuntimeError("Current-run STOP message received")
    return assignment, lease


def stop_child_tree(child: subprocess.Popen) -> None:
    if child.poll() is not None:
        return
    # Restrict cancellation to this wrapper's own child and descendants.
    subprocess.run(
        ["taskkill", "/PID", str(child.pid), "/T", "/F"],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def run(lane: str, command: list[str]) -> int:
    if os.name != "nt":
        raise RuntimeError("The team-v2 focused build slot requires Windows")
    assignment, lease = check_authorization(lane)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateMutexW.argtypes = (ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR)
    kernel.CreateMutexW.restype = wintypes.HANDLE
    kernel.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.ReleaseMutex.argtypes = (wintypes.HANDLE,)
    kernel.ReleaseMutex.restype = wintypes.BOOL
    kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel.CloseHandle.restype = wintypes.BOOL
    handle = kernel.CreateMutexW(None, False, MUTEX_NAME)
    if not handle:
        raise OSError(ctypes.get_last_error(), "CreateMutexW failed")
    held = False
    child = None
    try:
        print(f"{lane}: waiting for automatic focused build slot", flush=True)
        while True:
            check_authorization(lane)
            result = kernel.WaitForSingleObject(handle, 5000)
            if result in (WAIT_OBJECT_0, WAIT_ABANDONED):
                held = True
                break
            if result != WAIT_TIMEOUT:
                raise OSError(ctypes.get_last_error(), "WaitForSingleObject failed")
        check_authorization(lane)
        print(f"{lane}: focused build slot acquired", flush=True)
        child = subprocess.Popen(command, cwd=assignment["worktree"])
        while child.poll() is None:
            try:
                check_authorization(lane)
            except Exception:
                stop_child_tree(child)
                raise
            time.sleep(2)
        return child.returncode
    finally:
        if child is not None:
            stop_child_tree(child)
        if held:
            kernel.ReleaseMutex(handle)
            print(f"{lane}: focused build slot released", flush=True)
        kernel.CloseHandle(handle)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lane", required=True, choices=("scripts", "runtime", "actors", "assets", "world"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("Provide one build command after --")
    try:
        return run(args.lane, command)
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print(f"Focused build rejected: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
