"""Run an authorized team build without waiting for a coordinator grant.

The default is nonblocking: exit 75 means the resource is busy and no command
started. Use --wait only when this result is the next useful step. Each command
and its descendants belong to a Windows job, so closing or killing this wrapper
also stops its build. Coordination files remain the source of authorization.
"""

from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
from dataclasses import dataclass
from functools import lru_cache
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
BUSY = 75
POLL_SECONDS = 0.5
READ_ATTEMPTS = 6
READ_RETRY_SECONDS = 0.05
# These names are deliberately independent of the run and team generation.
MUTEX_NAMES = {
    "focused": "Global\\RustFalloutEngineFocusedBuild",
    "heavy": "Global\\RustFalloutEngineHeavyBuild",
}
WAIT_OBJECT_0 = 0
WAIT_ABANDONED = 0x80
WAIT_TIMEOUT = 0x102
INFINITE = 0xFFFFFFFF


def read_text(path: Path) -> str:
    for attempt in range(READ_ATTEMPTS):
        try:
            # PowerShell writers may include a BOM. Both forms are valid here.
            return path.read_text(encoding="utf-8-sig")
        except (PermissionError, FileNotFoundError):
            if attempt + 1 == READ_ATTEMPTS:
                raise
            time.sleep(READ_RETRY_SECONDS)
    raise AssertionError("unreachable coordination read")


def read_json(path: Path) -> dict:
    value = json.loads(read_text(path))
    if not isinstance(value, dict):
        raise ValueError(f"Expected a JSON object: {path}")
    return value


def outbox_rows(path: Path):
    for attempt in range(READ_ATTEMPTS):
        lines = read_text(path).split("\n")
        for line in lines[:-1]:
            if line.strip():
                row = json.loads(line)
                if not isinstance(row, dict):
                    raise ValueError(f"Expected JSON objects in {path}")
                yield row
        if not lines[-1].strip():
            return
        try:
            final = json.loads(lines[-1])
        except json.JSONDecodeError:
            # A concurrent append can expose a partial final row. Completed
            # STOP rows above take effect immediately; corruption cannot hide
            # forever behind the missing newline.
            if attempt + 1 == READ_ATTEMPTS:
                raise ValueError(f"Incomplete or malformed final outbox row: {path}")
            time.sleep(READ_RETRY_SECONDS)
            continue
        if not isinstance(final, dict):
            raise ValueError(f"Expected JSON objects in {path}")
        yield final
        return


def required_text(value: dict, field: str) -> str:
    text = value.get(field)
    if not isinstance(text, str) or not text.strip():
        raise ValueError(f"Missing or empty {field}")
    return text


def absolute_path(value: dict, field: str) -> Path:
    path = Path(required_text(value, field))
    if not path.is_absolute():
        raise ValueError(f"{field} must be an absolute path")
    return path.resolve()


def current_branch(worktree: Path) -> str:
    result = subprocess.run(
        ["git", "symbolic-ref", "--quiet", "--short", "HEAD"],
        cwd=worktree,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError("Assigned worktree must have its assigned branch checked out")
    return result.stdout.strip()


@dataclass(frozen=True)
class Authorization:
    generation: str
    run_id: str
    session_uuid: str
    lane: str
    worktree: Path
    branch: str
    target: Path
    slot: str


def check_authorization(
    lane: str, session: str, slot: str, expected: Authorization | None = None
) -> Authorization:
    if not re.fullmatch(r"[a-z][a-z0-9_-]*", lane):
        raise ValueError("Invalid lane name")
    if not session.strip():
        raise ValueError("A caller session UUID is required")
    if slot not in MUTEX_NAMES:
        raise ValueError("Unknown resource slot")

    control = read_json(ROOT / "local" / "team" / "control.json")
    team = absolute_path(control, "active_coordination_directory")
    if team != (ROOT / "local" / "team-v3").resolve():
        raise RuntimeError("This wrapper requires the central team-v3 directory")
    if control.get("mode") != "active" or control.get("stop_requested") is not False:
        raise RuntimeError("Team is stopped or inactive")
    if control.get("focused_build_policy") != "automatic_mutex":
        raise RuntimeError("Automatic build slots are not enabled")
    workers = control.get("workers")
    if not isinstance(workers, list) or not all(isinstance(item, str) for item in workers):
        raise ValueError("Control must name the worker lanes")
    if lane not in ["coordinator", *workers]:
        raise RuntimeError(f"{lane} is not a current lane")

    assignment = read_json(team / f"{lane}.assignment.json")
    lease = read_json(team / "leases" / f"{lane}.json")
    status = read_json(team / f"{lane}.status.json")
    if assignment.get("state") != "active" or assignment.get("implementation_authorized") is not True:
        raise RuntimeError(f"{lane} assignment is inactive")
    if status.get("state") not in ("active", "working", "building", "testing", "waiting_for_build"):
        raise RuntimeError(f"{lane} status is not active")
    allowed_slots = assignment.get("allowed_resource_slots")
    if not isinstance(allowed_slots, list) or slot not in allowed_slots:
        raise RuntimeError(f"{lane} is not assigned the {slot} resource slot")

    generation = required_text(control, "generation")
    run_id = required_text(control, "run_id")
    for record in (assignment, lease, status):
        if required_text(record, "generation") != generation:
            raise RuntimeError(f"{lane} generation differs from active control")
        if required_text(record, "run_id") != run_id:
            raise RuntimeError(f"{lane} run_id differs from active control")
        if required_text(record, "lane") != lane:
            raise RuntimeError(f"{lane} coordination belongs to another lane")
    for record in (lease, status):
        if required_text(record, "session_uuid") != session:
            raise RuntimeError(f"{lane} caller does not hold the current lease")

    primary = absolute_path(assignment, "worktree")
    primary_branch = required_text(assignment, "branch")
    if absolute_path(lease, "worktree") != primary or required_text(lease, "branch") != primary_branch:
        raise RuntimeError("Lease differs from the assigned primary worktree or branch")
    worktree = Path.cwd().resolve()
    if worktree == primary:
        branch = primary_branch
        target = absolute_path(assignment, "cargo_target_directory")
    elif lane == "coordinator" and worktree == absolute_path(assignment, "candidate_worktree"):
        branch = required_text(assignment, "candidate_branch")
        target = absolute_path(assignment, "candidate_target_directory")
    else:
        raise RuntimeError(f"Run from the assigned worktree: {primary}")
    if current_branch(worktree) != branch:
        raise RuntimeError(f"Worktree is not on assigned branch {branch}")
    configured_target = os.environ.get("CARGO_TARGET_DIR", "")
    if not configured_target or not Path(configured_target).is_absolute() or Path(configured_target).resolve() != target:
        raise RuntimeError("Set CARGO_TARGET_DIR to the assigned private target directory")

    for outbox in team.glob("*.outbox.jsonl"):
        for row in outbox_rows(outbox):
            if row.get("run_id") == run_id and row.get("type") == "stop_requested":
                raise RuntimeError("Current-run STOP message received")

    actual = Authorization(generation, run_id, session, lane, worktree, branch, target, slot)
    if expected is not None and actual != expected:
        raise RuntimeError("Build authorization changed while this command was running")
    return actual


class STARTUPINFO(ctypes.Structure):
    _fields_ = [
        ("cb", wintypes.DWORD), ("lpReserved", wintypes.LPWSTR),
        ("lpDesktop", wintypes.LPWSTR), ("lpTitle", wintypes.LPWSTR),
        ("dwX", wintypes.DWORD), ("dwY", wintypes.DWORD),
        ("dwXSize", wintypes.DWORD), ("dwYSize", wintypes.DWORD),
        ("dwXCountChars", wintypes.DWORD), ("dwYCountChars", wintypes.DWORD),
        ("dwFillAttribute", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
        ("wShowWindow", wintypes.WORD), ("cbReserved2", wintypes.WORD),
        ("lpReserved2", ctypes.POINTER(wintypes.BYTE)),
        ("hStdInput", wintypes.HANDLE), ("hStdOutput", wintypes.HANDLE),
        ("hStdError", wintypes.HANDLE),
    ]


class STARTUPINFOEX(ctypes.Structure):
    _fields_ = [("StartupInfo", STARTUPINFO), ("lpAttributeList", ctypes.c_void_p)]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("hProcess", wintypes.HANDLE), ("hThread", wintypes.HANDLE),
        ("dwProcessId", wintypes.DWORD), ("dwThreadId", wintypes.DWORD),
    ]


class SECURITY_ATTRIBUTES(ctypes.Structure):
    _fields_ = [
        ("nLength", wintypes.DWORD), ("lpSecurityDescriptor", ctypes.c_void_p),
        ("bInheritHandle", wintypes.BOOL),
    ]


class JOB_BASIC_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("PerProcessUserTimeLimit", ctypes.c_longlong),
        ("PerJobUserTimeLimit", ctypes.c_longlong), ("LimitFlags", wintypes.DWORD),
        ("MinimumWorkingSetSize", ctypes.c_size_t), ("MaximumWorkingSetSize", ctypes.c_size_t),
        ("ActiveProcessLimit", wintypes.DWORD), ("Affinity", ctypes.c_size_t),
        ("PriorityClass", wintypes.DWORD), ("SchedulingClass", wintypes.DWORD),
    ]


class IO_COUNTERS(ctypes.Structure):
    _fields_ = [(field, ctypes.c_ulonglong) for field in (
        "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
        "ReadTransferCount", "WriteTransferCount", "OtherTransferCount",
    )]


class JOB_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("BasicLimitInformation", JOB_BASIC_LIMIT_INFORMATION), ("IoInfo", IO_COUNTERS),
        ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
        ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t),
    ]


@lru_cache(maxsize=1)
def windows_api():
    if os.name != "nt":
        raise RuntimeError("Team build slots require Windows")
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    signatures = {
        "CreateMutexW": ([ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR], wintypes.HANDLE),
        "WaitForSingleObject": ([wintypes.HANDLE, wintypes.DWORD], wintypes.DWORD),
        "ReleaseMutex": ([wintypes.HANDLE], wintypes.BOOL),
        "CloseHandle": ([wintypes.HANDLE], wintypes.BOOL),
        "CreateJobObjectW": ([ctypes.c_void_p, wintypes.LPCWSTR], wintypes.HANDLE),
        "SetInformationJobObject": ([wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD], wintypes.BOOL),
        "InitializeProcThreadAttributeList": ([ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(ctypes.c_size_t)], wintypes.BOOL),
        "UpdateProcThreadAttribute": ([ctypes.c_void_p, wintypes.DWORD, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_void_p], wintypes.BOOL),
        "DeleteProcThreadAttributeList": ([ctypes.c_void_p], None),
        "CreateProcessW": ([wintypes.LPCWSTR, wintypes.LPWSTR, ctypes.c_void_p, ctypes.c_void_p, wintypes.BOOL, wintypes.DWORD, ctypes.c_void_p, wintypes.LPCWSTR, ctypes.POINTER(STARTUPINFOEX), ctypes.POINTER(PROCESS_INFORMATION)], wintypes.BOOL),
        "ResumeThread": ([wintypes.HANDLE], wintypes.DWORD),
        "GetExitCodeProcess": ([wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)], wintypes.BOOL),
        "GetStdHandle": ([wintypes.DWORD], wintypes.HANDLE),
        "GetCurrentProcess": ([], wintypes.HANDLE),
        "DuplicateHandle": ([wintypes.HANDLE, wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.HANDLE), wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.BOOL),
        "CreateFileW": ([wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(SECURITY_ATTRIBUTES), wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE], wintypes.HANDLE),
    }
    for name, (argtypes, restype) in signatures.items():
        function = getattr(kernel, name)
        function.argtypes = argtypes
        function.restype = restype
    return kernel


def win_error(operation: str) -> OSError:
    code = ctypes.get_last_error()
    return OSError(code, f"{operation}: {ctypes.FormatError(code).strip()}")


class WindowsMutex:
    def __init__(self, name: str):
        self.kernel = windows_api()
        self.handle = self.kernel.CreateMutexW(None, False, name)
        self.held = False
        if not self.handle:
            raise win_error("CreateMutexW")

    def __enter__(self):
        return self

    def acquire(self, milliseconds: int = 0) -> bool:
        if self.held:
            raise RuntimeError("Build mutex already held by this wrapper")
        result = self.kernel.WaitForSingleObject(self.handle, milliseconds)
        if result in (WAIT_OBJECT_0, WAIT_ABANDONED):
            self.held = True
            return True
        if result == WAIT_TIMEOUT:
            return False
        raise win_error("WaitForSingleObject")

    def __exit__(self, *_):
        if self.held:
            self.kernel.ReleaseMutex(self.handle)
        self.kernel.CloseHandle(self.handle)


class WindowsChild:
    """Own a process tree from creation, including an unexpected wrapper exit."""

    def __init__(self, command: list[str], cwd: Path):
        self.kernel = windows_api()
        self.process = None
        self.pid = None
        self.job = self.kernel.CreateJobObjectW(None, None)
        if not self.job:
            raise win_error("CreateJobObjectW")
        try:
            limits = JOB_EXTENDED_LIMIT_INFORMATION()
            limits.BasicLimitInformation.LimitFlags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            if not self.kernel.SetInformationJobObject(self.job, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
                raise win_error("SetInformationJobObject")
            self._start(command, cwd)
        except BaseException:
            self.close()
            raise

    def _start(self, command: list[str], cwd: Path) -> None:
        size = ctypes.c_size_t()
        self.kernel.InitializeProcThreadAttributeList(None, 2, 0, ctypes.byref(size))
        if not size.value:
            raise win_error("Size process attribute list")
        storage = ctypes.create_string_buffer(size.value)
        if not self.kernel.InitializeProcThreadAttributeList(storage, 2, 0, ctypes.byref(size)):
            raise win_error("InitializeProcThreadAttributeList")
        handles = []
        try:
            jobs = (wintypes.HANDLE * 1)(self.job)
            # JOB_LIST assigns ownership as part of CreateProcess. Creating a
            # suspended process and assigning it later leaves an orphan window.
            if not self.kernel.UpdateProcThreadAttribute(storage, 0, 0x0002000D, jobs, ctypes.sizeof(jobs), None, None):
                raise win_error("Set process job list (requires Windows 10 or later)")
            handles = self._standard_handles()
            inherited = (wintypes.HANDLE * 3)(*handles)
            if not self.kernel.UpdateProcThreadAttribute(storage, 0, 0x00020002, inherited, ctypes.sizeof(inherited), None, None):
                raise win_error("Set inherited standard handles")
            startup = STARTUPINFOEX()
            startup.StartupInfo.cb = ctypes.sizeof(startup)
            startup.lpAttributeList = ctypes.cast(storage, ctypes.c_void_p)
            startup.StartupInfo.dwFlags = 0x100  # STARTF_USESTDHANDLES
            startup.StartupInfo.hStdInput, startup.StartupInfo.hStdOutput, startup.StartupInfo.hStdError = handles
            information = PROCESS_INFORMATION()
            arguments = ctypes.create_unicode_buffer(subprocess.list2cmdline(command))
            flags = 0x00000004 | 0x00080000 | 0x08000000  # suspended, extended startup, no new console
            if not self.kernel.CreateProcessW(None, arguments, None, None, True, flags, None, str(cwd), ctypes.byref(startup), ctypes.byref(information)):
                raise win_error("CreateProcessW")
            self.process = information.hProcess
            self.pid = information.dwProcessId
            try:
                if self.kernel.ResumeThread(information.hThread) == 0xFFFFFFFF:
                    raise win_error("ResumeThread")
            finally:
                self.kernel.CloseHandle(information.hThread)
        finally:
            self.kernel.DeleteProcThreadAttributeList(storage)
            for handle in handles:
                self.kernel.CloseHandle(handle)

    def _standard_handles(self) -> list:
        handles = []
        process = self.kernel.GetCurrentProcess()
        try:
            for index, value in enumerate((-10, -11, -12)):
                original = self.kernel.GetStdHandle(value & 0xFFFFFFFF)
                if original and original != ctypes.c_void_p(-1).value:
                    copied = wintypes.HANDLE()
                    if not self.kernel.DuplicateHandle(process, original, process, ctypes.byref(copied), 0, True, 2):
                        raise win_error("Duplicate standard handle")
                    handles.append(copied.value)
                else:
                    # Headless hosts may have no stdin. Supply only that missing
                    # stream with NUL rather than dropping captured stdout/stderr.
                    security = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)
                    access = 0x80000000 if index == 0 else 0x40000000
                    handle = self.kernel.CreateFileW("NUL", access, 3, ctypes.byref(security), 3, 0x80, None)
                    if handle == ctypes.c_void_p(-1).value:
                        raise win_error("Open missing standard stream")
                    handles.append(handle)
            return handles
        except BaseException:
            for handle in handles:
                self.kernel.CloseHandle(handle)
            raise

    def poll(self) -> int | None:
        result = self.kernel.WaitForSingleObject(self.process, 0)
        if result == WAIT_TIMEOUT:
            return None
        if result != WAIT_OBJECT_0:
            raise win_error("WaitForSingleObject(child)")
        code = wintypes.DWORD()
        if not self.kernel.GetExitCodeProcess(self.process, ctypes.byref(code)):
            raise win_error("GetExitCodeProcess")
        return code.value

    def close(self) -> None:
        # The job handle is not inheritable. Closing its final handle kills any
        # remaining descendants even when the command's top-level process exited.
        if self.job:
            self.kernel.CloseHandle(self.job)
            self.job = None
        if self.process:
            self.kernel.WaitForSingleObject(self.process, 5000)
            self.kernel.CloseHandle(self.process)
            self.process = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def run(lane: str, session: str, slot: str, command: list[str], wait: bool = False) -> int:
    if not command:
        raise ValueError("Provide a command to run")
    authorization = check_authorization(lane, session, slot)
    with WindowsMutex(MUTEX_NAMES[slot]) as mutex:
        while True:
            check_authorization(lane, session, slot, authorization)
            if mutex.acquire(0):
                break
            check_authorization(lane, session, slot, authorization)
            if not wait:
                print(f"{lane}: {slot} slot busy; no command started (exit {BUSY})", flush=True)
                return BUSY
            time.sleep(POLL_SECONDS)
        check_authorization(lane, session, slot, authorization)
        print(f"{lane}: {slot} slot acquired", flush=True)
        with WindowsChild(command, authorization.worktree) as child:
            while True:
                check_authorization(lane, session, slot, authorization)
                result = child.poll()
                if result is not None:
                    return result
                time.sleep(POLL_SECONDS)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lane", required=True)
    parser.add_argument("--session", required=True, help="UUID of the caller's current lane lease")
    parser.add_argument("--slot", choices=tuple(MUTEX_NAMES), default="focused")
    parser.add_argument("--wait", action="store_true", help="Wait for the slot while checking STOP")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("Provide one command after --")
    try:
        return run(args.lane, args.session, args.slot, command, args.wait)
    except KeyboardInterrupt:
        print("Build cancelled", file=sys.stderr)
        return 130
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print(f"Build rejected: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
