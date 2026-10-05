"""Private actual-token/process proof using authored canaries; no original launch."""
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

ROOT = Path(r"G:\Rust-Fallout")
EVIDENCE = Path(__file__).parent
OUT = EVIDENCE / "appcontainer-canary-proof-27-attempt-01"
RUN = "team-v4-20261004-20261004T222612Z-0f8c3e22"
RUST = ROOT / ".tools/rustup/toolchains/1.99.0-x86_64-pc-windows-msvc/bin/rustc.exe"

def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))

def guard():
    c = read(ROOT / "local/team/control.json")
    a = read(ROOT / "local/team-v4/coordinator.assignment.json")
    l = read(ROOT / "local/team-v4/leases/coordinator.json")
    assert c["mode"] == a["state"] == "active" and not c["stop_requested"] and not a["stop_requested"]
    assert c["run_id"] == a["run_id"] == l["run_id"] == RUN
    assert l["session_uuid"] == "9db82ecf-abfa-4024-a4c1-e6e4830b0d37"

def pin(path):
    raw = path.read_bytes()
    return dict(path=str(path), bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())

def save():
    guard()
    (OUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8", newline="\n")

spec = importlib.util.spec_from_file_location("fallout_team_build_canary", ROOT / "tools/team-build.py")
team = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = team
spec.loader.exec_module(team)
kernel = team.windows_api()
advapi = ctypes.WinDLL("advapi32", use_last_error=True)
userenv = ctypes.WinDLL("userenv", use_last_error=True)
ole = ctypes.WinDLL("ole32", use_last_error=True)
advapi.OpenProcessToken.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE)]
advapi.OpenProcessToken.restype = wintypes.BOOL
advapi.GetTokenInformation.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
advapi.GetTokenInformation.restype = wintypes.BOOL
advapi.ConvertSidToStringSidW.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
advapi.ConvertSidToStringSidW.restype = wintypes.BOOL
advapi.FreeSid.argtypes = [ctypes.c_void_p]
advapi.FreeSid.restype = ctypes.c_void_p
kernel.LocalFree.argtypes = [ctypes.c_void_p]
kernel.LocalFree.restype = ctypes.c_void_p
kernel.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
kernel.QueryFullProcessImageNameW.restype = wintypes.BOOL
userenv.CreateAppContainerProfile.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.LPCWSTR, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(ctypes.c_void_p)]
userenv.CreateAppContainerProfile.restype = ctypes.c_long
userenv.GetAppContainerFolderPath.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(ctypes.c_void_p)]
userenv.GetAppContainerFolderPath.restype = ctypes.c_long
ole.CoTaskMemFree.argtypes = [ctypes.c_void_p]
ole.CoTaskMemFree.restype = None

def sid_text(sid):
    output = ctypes.c_void_p()
    if not advapi.ConvertSidToStringSidW(sid, ctypes.byref(output)):
        raise team.win_error("ConvertSidToStringSidW")
    try:
        value = ctypes.wstring_at(output)
        assert len(value) < 1024 and value.startswith("S-1-15-2-")
        return value
    finally:
        kernel.LocalFree(output)

def token_identity(process):
    token = wintypes.HANDLE()
    if not advapi.OpenProcessToken(process, 8, ctypes.byref(token)):
        raise team.win_error("OpenProcessToken(child)")
    try:
        container = wintypes.DWORD()
        returned = wintypes.DWORD()
        if not advapi.GetTokenInformation(token, 29, ctypes.byref(container), 4, ctypes.byref(returned)):
            raise team.win_error("GetTokenInformation(AppContainer)")
        assert container.value == 1 and returned.value == 4
        storage = (ctypes.c_uint64 * 512)()
        if not advapi.GetTokenInformation(token, 31, storage, ctypes.sizeof(storage), ctypes.byref(returned)):
            raise team.win_error("GetTokenInformation(AppContainer SID)")
        assert ctypes.sizeof(ctypes.c_void_p) <= returned.value <= ctypes.sizeof(storage)
        sid = ctypes.cast(storage, ctypes.POINTER(ctypes.c_void_p))[0]
        return dict(is_appcontainer=1, appcontainer_sid=sid_text(sid))
    finally:
        kernel.CloseHandle(token)

class Capabilities(ctypes.Structure):
    _fields_ = [("AppContainerSid", ctypes.c_void_p), ("Capabilities", ctypes.c_void_p),
                ("CapabilityCount", wintypes.DWORD), ("Reserved", wintypes.DWORD)]

class ContainerChild(team.WindowsChild):
    def __init__(self, command, cwd, sid, expected_sid, stdout, stderr):
        self.capabilities = Capabilities(sid, None, 0, 0)
        self.expected_sid = expected_sid
        self.expected_image = Path(command[0]).resolve()
        self.stdout_path = stdout
        self.stderr_path = stderr
        self.observation = None
        super().__init__(command, cwd)

    def _standard_handles(self):
        security = team.SECURITY_ATTRIBUTES(ctypes.sizeof(team.SECURITY_ATTRIBUTES), None, True)
        handles = []
        try:
            for path, access, disposition in [("NUL", 0x80000000, 3), (str(self.stdout_path), 0x40000000, 1), (str(self.stderr_path), 0x40000000, 1)]:
                handle = self.kernel.CreateFileW(path, access, 1, ctypes.byref(security), disposition, 0x80, None)
                if handle == ctypes.c_void_p(-1).value:
                    raise team.win_error("Open owned canary stdio")
                handles.append(handle)
            return handles
        except BaseException:
            for handle in handles:
                self.kernel.CloseHandle(handle)
            raise

    def _start(self, command, cwd):
        guard()
        size = ctypes.c_size_t()
        self.kernel.InitializeProcThreadAttributeList(None, 3, 0, ctypes.byref(size))
        assert 0 < size.value <= 65536
        storage = ctypes.create_string_buffer(size.value)
        if not self.kernel.InitializeProcThreadAttributeList(storage, 3, 0, ctypes.byref(size)):
            raise team.win_error("Initialize canary attributes")
        handles = []
        try:
            jobs = (wintypes.HANDLE * 1)(self.job)
            if not self.kernel.UpdateProcThreadAttribute(storage, 0, 0x2000D, jobs, ctypes.sizeof(jobs), None, None):
                raise team.win_error("Set canary owned job")
            handles = self._standard_handles()
            inherited = (wintypes.HANDLE * 3)(*handles)
            if not self.kernel.UpdateProcThreadAttribute(storage, 0, 0x20002, inherited, ctypes.sizeof(inherited), None, None):
                raise team.win_error("Set canary stdio allowlist")
            if not self.kernel.UpdateProcThreadAttribute(storage, 0, 0x20009, ctypes.byref(self.capabilities), ctypes.sizeof(self.capabilities), None, None):
                raise team.win_error("Set canary AppContainer capabilities")
            startup = team.STARTUPINFOEX()
            startup.StartupInfo.cb = ctypes.sizeof(startup)
            startup.lpAttributeList = ctypes.cast(storage, ctypes.c_void_p)
            startup.StartupInfo.dwFlags = 0x100
            startup.StartupInfo.hStdInput, startup.StartupInfo.hStdOutput, startup.StartupInfo.hStdError = handles
            info = team.PROCESS_INFORMATION()
            args = ctypes.create_unicode_buffer(subprocess.list2cmdline(command))
            if not self.kernel.CreateProcessW(str(self.expected_image), args, None, None, True, 0x4 | 0x80000 | 0x8000000, None, str(cwd), ctypes.byref(startup), ctypes.byref(info)):
                raise team.win_error("Create authored AppContainer process")
            self.process = info.hProcess
            self.pid = info.dwProcessId
            try:
                identity = token_identity(self.process)
                assert identity["appcontainer_sid"] == self.expected_sid
                image = ctypes.create_unicode_buffer(32768)
                count = wintypes.DWORD(32768)
                if not kernel.QueryFullProcessImageNameW(self.process, 0, image, ctypes.byref(count)):
                    raise team.win_error("Query actual canary image")
                assert Path(image.value).resolve() == self.expected_image
                self.observation = dict(pid=self.pid, image=image.value, verified_before_resume=True, **identity)
                guard()
                if kernel.ResumeThread(info.hThread) == 0xFFFFFFFF:
                    raise team.win_error("Resume authored canary")
            finally:
                kernel.CloseHandle(info.hThread)
        finally:
            self.kernel.DeleteProcThreadAttributeList(storage)
            for handle in handles:
                self.kernel.CloseHandle(handle)

guard()
OUT.mkdir(exist_ok=False)
receipt = dict(schema_version=1, run_id=RUN, state="building_authored_probe", started_at=datetime.now(timezone.utc).isoformat(), sources=[pin(Path(__file__)), pin(EVIDENCE / "appcontainer_canary_27.rs"), pin(ROOT / "tools/team-build.py")], original_process_launched=False, existing_acls_changed=False, cases=[], scope="Native authored AppContainer token/private IO/owned cancellation only; original isolation and game compatibility remain open")
save()
sid = ctypes.c_void_p()
try:
    binary = OUT / "canary.exe"
    command = [str(RUST), "--edition=2024", "-D", "warnings", str(EVIDENCE / "appcontainer_canary_27.rs"), "-o", str(binary)]
    with (OUT / "build.log").open("xb") as log:
        result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
    receipt["build"] = dict(command=command, exit_code=result.returncode, log=pin(OUT / "build.log"))
    save()
    assert result.returncode == 0
    receipt["compiled_binary"] = pin(binary)
    name = "RustFallout.Probe." + uuid.uuid4().hex
    receipt["profile_name"] = name
    receipt["state"] = "creating_private_profile"
    save()
    guard()
    hr = userenv.CreateAppContainerProfile(name, name, "Authored Rust Fallout process isolation canary", None, 0, ctypes.byref(sid))
    receipt["profile_create_hresult"] = hr & 0xFFFFFFFF
    save()
    assert hr == 0 and sid.value, hex(hr & 0xFFFFFFFF)
    sid_string = sid_text(sid)
    receipt["profile_created"] = True
    receipt["profile_sid"] = sid_string
    save()
    raw_folder = ctypes.c_void_p()
    hr = userenv.GetAppContainerFolderPath(sid_string, ctypes.byref(raw_folder))
    assert hr == 0 and raw_folder.value
    try:
        folder = Path(ctypes.wstring_at(raw_folder)).resolve(strict=True)
    finally:
        ole.CoTaskMemFree(raw_folder)
    packages = (Path(os.environ["LOCALAPPDATA"]) / "Packages").resolve(strict=True)
    assert folder.is_relative_to(packages) and name.casefold() in [part.casefold() for part in folder.parts]
    receipt["profile_folder"] = str(folder)
    # Every write is under the freshly created, uniquely named profile or OUT.
    work = folder / "AC" if (folder / "AC").is_dir() else folder
    private = work / ("RustFalloutCanary-" + uuid.uuid4().hex)
    guard()
    private.mkdir(exist_ok=False)
    image = private / "canary.exe"
    shutil.copyfile(binary, image)
    assert pin(image)["sha256"] == pin(binary)["sha256"]
    input_file = private / "input.txt"
    with input_file.open("xb") as stream:
        stream.write(b"rust-fallout-private-canary-v1\n")
    protected = OUT / "protected-host-canary.txt"
    with protected.open("xb") as stream:
        stream.write(b"authored host canary must remain unchanged\n")
    protected_before = pin(protected)
    input_before = pin(input_file)
    image_before = pin(image)
    receipt["state"] = "running_authored_cases"
    receipt["executed_image_before"] = image_before
    save()
    for mode in ["normal", "linger"]:
        guard()
        stdout = OUT / (mode + ".stdout.json")
        stderr = OUT / (mode + ".stderr.log")
        output = private / (mode + "-write.txt")
        with ContainerChild([str(image), str(input_file), str(output), str(protected), mode], private, sid.value, sid_string, stdout, stderr) as child:
            row = dict(mode=mode, parent_observation=child.observation)
            receipt["cases"].append(row)
            save()
            deadline = time.monotonic() + 20
            if mode == "normal":
                while child.poll() is None:
                    guard()
                    assert time.monotonic() < deadline, "authored normal canary deadline"
                    time.sleep(0.05)
                row["exit_code"] = child.poll()
                assert row["exit_code"] == 0
            else:
                while not (stdout.exists() and stdout.stat().st_size > 0):
                    guard()
                    assert child.poll() is None and time.monotonic() < deadline
                    time.sleep(0.05)
                assert child.poll() is None
                guard()
                kernel.CloseHandle(child.job)
                child.job = None
                assert kernel.WaitForSingleObject(child.process, 5000) == 0
                row["exit_code"] = child.poll()
                row["owned_job_cancellation_observed"] = True
            assert stdout.stat().st_size <= 65536 and stderr.stat().st_size <= 65536
            observed = read(stdout)
            assert observed["pid"] == child.pid and observed["is_appcontainer"] == 1 and observed["appcontainer_sid"] == sid_string
            assert observed["private_read"] and observed["private_write"] and observed["protected_write_open_error"] == 5
            assert output.read_bytes() == input_file.read_bytes()
            assert pin(protected) == protected_before and pin(input_file) == input_before and pin(image) == image_before
            row.update(native_observation=observed, stdout=pin(stdout), stderr=pin(stderr), private_written_file=pin(output))
            save()
    receipt.update(state="authored_transport_passed", executed_image_after=pin(image), protected_host_canary_unchanged=True, completed_at=datetime.now(timezone.utc).isoformat(), profile_preserved_for_followup=True, limitations=["64-bit authored native child only; original32-bit executable not launched", "Known-folder paths are observations; no original document redirection/retail compatibility acceptance", "Private stdout handles are the only inherited writable host handles; canaries do not access installed inputs or real saves", "New AppContainer profile remains preserved; no existing profile/ACL/settings altered"])
    save()
except BaseException as error:
    receipt.update(state="failed_preserved", error=f"{type(error).__name__}: {error}")
    save()
    raise
finally:
    if sid.value:
        advapi.FreeSid(sid)
print(json.dumps(dict(state=receipt["state"], receipt=pin(OUT / "receipt.json"), original_process_launched=False)), flush=True)
