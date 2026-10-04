"""Optional team authorization, short owned-process polls and STOP cancellation."""
import json
import os
from pathlib import Path
import subprocess


def guard():
    session = os.environ.get("ASSET_TEAM_SESSION_UUID")
    if not session:
        return
    team = Path(os.environ["ASSET_TEAM_DIRECTORY"])
    control = json.loads(Path(os.environ["ASSET_TEAM_CONTROL"]).read_text(encoding="utf-8-sig"))
    assignment = json.loads((team / "assets.assignment.json").read_text(encoding="utf-8-sig"))
    lease = json.loads((team / "leases/assets.json").read_text(encoding="utf-8-sig"))
    if (control.get("generation") != "team-v2-20261003" or control.get("mode") != "active"
            or control.get("stop_requested") or assignment.get("state") != "active"
            or not assignment.get("implementation_authorized") or assignment.get("generation") != control["generation"]
            or assignment.get("run_id") != control.get("run_id") or lease.get("session_uuid") != session):
        raise RuntimeError("STOP/inactive/mismatched assets coordination; preserve current output")
    for path in team.glob("*.outbox.jsonl"):
        for line in path.read_text(encoding="utf-8-sig").splitlines():
            if line.strip():
                row = json.loads(line)
                if row.get("run_id") == control["run_id"] and row.get("type") == "stop_requested":
                    raise RuntimeError("Current-run STOP row; preserve current output")


def run(command, **kwargs):
    if kwargs.pop("capture_output", False):
        kwargs["stdout"] = subprocess.PIPE
        kwargs["stderr"] = subprocess.PIPE
    check = kwargs.pop("check", False)
    guard()
    process = subprocess.Popen(command, **kwargs)
    try:
        while True:
            guard()
            try:
                stdout, stderr = process.communicate(timeout=5)
                guard()
                result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
                if check and result.returncode:
                    raise subprocess.CalledProcessError(result.returncode, command, stdout, stderr)
                return result
            except subprocess.TimeoutExpired:
                pass
    except BaseException:
        if process.poll() is None:
            process.terminate()
            try:
                process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate(timeout=5)
        raise
