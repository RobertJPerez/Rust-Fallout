"""Authorization and Windows lifetime checks using private temporary fixtures."""

from __future__ import annotations

from contextlib import ExitStack
import ctypes
from ctypes import wintypes
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
import uuid


SOURCE = Path(__file__).with_name("team-build.py")
SPEC = importlib.util.spec_from_file_location("team_build", SOURCE)
assert SPEC is not None and SPEC.loader is not None
build = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = build
SPEC.loader.exec_module(build)


class AuthorizationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="fallout-team-build-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.team = self.root / "local" / "team-v3"
        (self.team / "leases").mkdir(parents=True)
        (self.root / "local" / "team").mkdir()
        self.worktree = self.root / "worker"
        self.worktree.mkdir()
        self.control = {
            "mode": "active", "stop_requested": False,
            "focused_build_policy": "automatic_mutex",
            "active_coordination_directory": str(self.team),
            "generation": "team-v3-test", "run_id": "run-test",
            "workers": ["scripts", "runtime", "actors", "assets", "world", "presentation", "physics"],
        }
        identity = {"generation": "team-v3-test", "run_id": "run-test", "lane": "scripts"}
        self.assignment = {
            **identity, "state": "active", "implementation_authorized": True,
            "worktree": str(self.worktree), "branch": "agents/scripts",
            "cargo_target_directory": str(self.worktree / "target"),
            "allowed_resource_slots": ["focused"],
        }
        self.lease = {
            **identity, "session_uuid": "test-session", "worktree": str(self.worktree),
            "branch": "agents/scripts",
        }
        self.status = {**identity, "session_uuid": "test-session", "state": "active"}
        self.lane = "scripts"
        self.save()
        self.addCleanup(patch.stopall)
        patch.object(build, "ROOT", self.root).start()
        patch.object(Path, "cwd", return_value=self.worktree).start()
        self.branch = patch.object(build, "current_branch", return_value="agents/scripts").start()
        patch.dict(os.environ, {"CARGO_TARGET_DIR": str(self.worktree / "target")}).start()

    def save(self, encoding="utf-8"):
        for path, value in (
            (self.root / "local" / "team" / "control.json", self.control),
            (self.team / f"{self.lane}.assignment.json", self.assignment),
            (self.team / "leases" / f"{self.lane}.json", self.lease),
            (self.team / f"{self.lane}.status.json", self.status),
        ):
            temporary = path.with_suffix(path.suffix + ".tmp")
            temporary.write_text(json.dumps(value), encoding=encoding)
            temporary.replace(path)

    def authorize(self, slot="focused", expected=None):
        return build.check_authorization(self.lane, "test-session", slot, expected)

    def test_active_fixture_and_bom_are_admitted(self):
        for encoding in ("utf-8", "utf-8-sig"):
            with self.subTest(encoding=encoding):
                self.save(encoding)
                self.assertEqual(self.authorize().worktree, self.worktree)

    def test_resource_budget_defaults_and_live_changes_preserve_identity(self):
        first = self.authorize()
        self.assertEqual(first.resource_capacity, 1)
        for capacity in (2, 1):
            self.control["maximum_focused_cargo_builds"] = capacity
            self.save()
            current = self.authorize(expected=first)
            self.assertEqual(current, first)
            self.assertEqual(current.resource_capacity, capacity)
        self.assignment["allowed_resource_slots"].append("heavy")
        self.control["maximum_focused_cargo_builds"] = 2
        self.save()
        self.assertEqual(self.authorize("heavy").resource_capacity, 1)

    def test_invalid_resource_budgets_refuse_before_launch(self):
        invalid = {
            "maximum_focused_cargo_builds": (None, True, False, "2", 0, 3, -1, 1.0, 2.0, []),
            "maximum_heavy_commands": (None, True, False, "1", 0, 2, -1, 1.0, []),
        }
        for field, values in invalid.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    self.control[field] = value
                    self.save()
                    with patch.object(build, "WindowsMutex") as mutex, patch.object(build, "WindowsChild") as child:
                        with self.assertRaisesRegex(ValueError, field):
                            build.run(self.lane, "test-session", "focused", ["unused"])
                        mutex.assert_not_called()
                        child.assert_not_called()
            self.control.pop(field)
            self.save()

    def test_paused_or_stop_control_refuses(self):
        for update in ({"mode": "paused"}, {"stop_requested": True}):
            with self.subTest(update=update):
                original = self.control.copy()
                self.control.update(update)
                self.save()
                with self.assertRaisesRegex(RuntimeError, "stopped or inactive"):
                    self.authorize()
                self.control = original

    def test_current_run_stop_refuses_but_historical_stop_does_not(self):
        outbox = self.team / "actors.outbox.jsonl"
        outbox.write_text(json.dumps({"run_id": "older-run", "type": "stop_requested"}) + "\n", encoding="utf-8-sig")
        self.authorize()
        with outbox.open("a", encoding="utf-8") as stream:
            stream.write(json.dumps({"run_id": "run-test", "type": "stop_requested"}) + "\n")
        with self.assertRaisesRegex(RuntimeError, "STOP"):
            self.authorize()

    def test_partial_outbox_append_retries_then_stop_refuses(self):
        outbox = self.team / "actors.outbox.jsonl"
        outbox.write_text('{"run_id":"run-test","type":"stop_requested"}', encoding="utf-8")
        original_read = build.read_text
        versions = iter(['{"run_id":"run-test","type":', original_read(outbox)])
        def concurrent_read(path):
            return next(versions) if path == outbox else original_read(path)
        with patch.object(build, "read_text", side_effect=concurrent_read), patch.object(build.time, "sleep") as sleep:
            with self.assertRaisesRegex(RuntimeError, "STOP"):
                self.authorize()
            sleep.assert_called_once()
        # A complete STOP is actionable even before the writer adds its newline.
        with self.assertRaisesRegex(RuntimeError, "STOP"):
            self.authorize()
        outbox.write_text('{not json}\n', encoding="utf-8")
        with self.assertRaises(json.JSONDecodeError):
            self.authorize()
        outbox.write_text('{not json}', encoding="utf-8")
        with patch.object(build.time, "sleep") as sleep:
            with self.assertRaisesRegex(ValueError, "final outbox row"):
                self.authorize()
            self.assertEqual(sleep.call_count, build.READ_ATTEMPTS - 1)

    def test_complete_stop_is_not_delayed_by_another_partial_row(self):
        outbox = self.team / "actors.outbox.jsonl"
        outbox.write_text('{"run_id":"run-test","type":"stop_requested"}\n{"partial":', encoding="utf-8")
        with patch.object(build.time, "sleep") as sleep:
            with self.assertRaisesRegex(RuntimeError, "STOP"):
                self.authorize()
            sleep.assert_not_called()

    def test_unicode_line_separator_inside_completed_message_is_not_a_row_boundary(self):
        outbox = self.team / "actors.outbox.jsonl"
        outbox.write_text(json.dumps({"message": "one\u2028two", "type": "progress"}, ensure_ascii=False) + "\n", encoding="utf-8")
        self.authorize()

    def test_lease_and_status_replacement_cannot_adopt_old_caller(self):
        expected = self.authorize()
        self.lease["session_uuid"] = self.status["session_uuid"] = "new-session"
        self.save()
        with self.assertRaisesRegex(RuntimeError, "caller does not hold"):
            self.authorize(expected=expected)

    def test_changed_run_cannot_resume_existing_wrapper(self):
        expected = self.authorize()
        for record in (self.control, self.assignment, self.lease, self.status):
            record["run_id"] = "new-run"
        self.save()
        with self.assertRaisesRegex(RuntimeError, "authorization changed"):
            self.authorize(expected=expected)

    def test_missing_and_mismatched_identity_refuses(self):
        for record in (self.assignment, self.lease, self.status):
            for field in ("run_id", "generation", "lane"):
                original = record[field]
                for value in (None, "wrong"):
                    with self.subTest(field=field, value=value):
                        record[field] = value
                        self.save()
                        with self.assertRaises((ValueError, RuntimeError)):
                            self.authorize()
                record[field] = original
                self.save()

    def test_inactive_assignment_or_status_refuses(self):
        self.assignment["implementation_authorized"] = False
        self.save()
        with self.assertRaisesRegex(RuntimeError, "assignment is inactive"):
            self.authorize()
        self.assignment["implementation_authorized"] = True
        self.status["state"] = "stopped"
        self.save()
        with self.assertRaisesRegex(RuntimeError, "status is not active"):
            self.authorize()

    def test_directory_escape_and_old_team_refuse(self):
        for path in (self.root / "outside", self.root / "local" / "team-v2"):
            self.control["active_coordination_directory"] = str(path)
            self.save()
            with self.assertRaisesRegex(RuntimeError, "central team-v3"):
                self.authorize()

    def test_wrong_branch_worktree_or_target_refuses(self):
        self.branch.return_value = "another-branch"
        with self.assertRaisesRegex(RuntimeError, "assigned branch"):
            self.authorize()
        self.branch.return_value = "agents/scripts"
        with patch.object(Path, "cwd", return_value=self.root):
            with self.assertRaisesRegex(RuntimeError, "assigned worktree"):
                self.authorize()
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": str(self.root / "shared-target")}):
            with self.assertRaisesRegex(RuntimeError, "private target"):
                self.authorize()

    def test_lease_worktree_must_match_assignment(self):
        self.lease["worktree"] = str(self.root)
        self.save()
        with self.assertRaisesRegex(RuntimeError, "Lease differs"):
            self.authorize()

    def test_heavy_slot_requires_explicit_assignment(self):
        with self.assertRaisesRegex(RuntimeError, "not assigned the heavy"):
            self.authorize("heavy")
        self.assignment["allowed_resource_slots"].append("heavy")
        self.save()
        self.assertEqual(self.authorize("heavy").slot, "heavy")

    def test_new_worker_lanes_are_read_from_control(self):
        for self.lane in ("presentation", "physics"):
            for record in (self.assignment, self.lease, self.status):
                record["lane"] = self.lane
            self.save()
            self.assertEqual(self.authorize().lane, self.lane)
        with self.assertRaisesRegex(ValueError, "Invalid lane"):
            build.check_authorization("../scripts", "test-session", "focused")

    def test_coordinator_candidate_uses_its_declared_branch_and_target(self):
        self.lane = "coordinator"
        candidate = self.root / "candidate"
        candidate.mkdir()
        for record in (self.assignment, self.lease, self.status):
            record["lane"] = self.lane
        self.assignment.update({
            "candidate_worktree": str(candidate), "candidate_branch": "agents/integration-next",
            "candidate_target_directory": str(candidate / "target"),
        })
        self.save()
        self.branch.return_value = "agents/integration-next"
        with patch.object(Path, "cwd", return_value=candidate), patch.dict(os.environ, {"CARGO_TARGET_DIR": str(candidate / "target")}):
            self.assertEqual(self.authorize().worktree, candidate)

    def test_busy_slot_never_launches_child(self):
        with patch.object(build, "WindowsMutex") as mutex_type, patch.object(build, "WindowsChild") as child_type:
            mutex = mutex_type.return_value.__enter__.return_value
            mutex.acquire.return_value = False
            self.assertEqual(build.run(self.lane, "test-session", "focused", ["unused"]), build.BUSY)
            child_type.assert_not_called()

    def test_waiting_wrapper_rechecks_stop_before_launch(self):
        with patch.object(build, "WindowsMutex") as mutex_type, patch.object(build, "WindowsChild") as child_type, patch.object(build.time, "sleep") as sleep:
            mutex = mutex_type.return_value.__enter__.return_value
            mutex.acquire.return_value = False
            def stop(_):
                self.control["stop_requested"] = True
                self.save()
            sleep.side_effect = stop
            with self.assertRaisesRegex(RuntimeError, "stopped or inactive"):
                build.run(self.lane, "test-session", "focused", ["unused"], wait=True)
            child_type.assert_not_called()

    def test_running_wrapper_cancels_its_owned_child_on_lease_replacement(self):
        with patch.object(build, "WindowsMutex") as mutex_type, patch.object(build, "WindowsChild") as child_type, patch.object(build.time, "sleep") as sleep:
            mutex_type.return_value.__enter__.return_value.acquire.return_value = True
            child_type.return_value.__enter__.return_value.poll.return_value = None
            def replace(_):
                self.lease["session_uuid"] = self.status["session_uuid"] = "replacement"
                self.save()
            sleep.side_effect = replace
            with self.assertRaisesRegex(RuntimeError, "caller does not hold"):
                build.run(self.lane, "test-session", "focused", ["unused"])
            child_type.return_value.__exit__.assert_called_once()
            mutex_type.return_value.__exit__.assert_called_once()

    def test_child_exit_and_launch_failure_release_slot(self):
        with patch.object(build, "WindowsMutex") as mutex_type, patch.object(build, "WindowsChild") as child_type:
            mutex_type.return_value.__enter__.return_value.acquire.return_value = True
            child_type.return_value.__enter__.return_value.poll.return_value = 37
            self.assertEqual(build.run(self.lane, "test-session", "focused", ["unused"]), 37)
            mutex_type.return_value.__exit__.assert_called_once()
        with patch.object(build, "WindowsMutex") as mutex_type, patch.object(build, "WindowsChild", side_effect=OSError("failed")):
            mutex_type.return_value.__enter__.return_value.acquire.return_value = True
            with self.assertRaises(OSError):
                build.run(self.lane, "test-session", "focused", ["unused"])
            mutex_type.return_value.__exit__.assert_called_once()

    def hold_test_mutex(self, name, command=None):
        ready, release = threading.Event(), threading.Event()
        failures = []

        def holder():
            try:
                with build.WindowsMutex(name) as mutex, ExitStack() as stack:
                    if not mutex.acquire():
                        raise RuntimeError("Private test mutex unexpectedly occupied")
                    if command is not None:
                        stack.enter_context(build.WindowsChild(command, self.root))
                    ready.set()
                    if not release.wait(30):
                        raise RuntimeError("Private test mutex holder timed out")
            except BaseException as error:
                failures.append(error)
            finally:
                ready.set()

        thread = threading.Thread(target=holder)
        thread.start()

        def cleanup():
            release.set()
            thread.join(5)
            self.assertFalse(thread.is_alive())
            self.assertEqual(failures, [])

        self.addCleanup(cleanup)
        self.assertTrue(ready.wait(5), "Private test mutex holder did not start")
        self.assertEqual(failures, [])
        return thread

    def await_test_json(self, path):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if path.exists():
                try:
                    return json.loads(path.read_text(encoding="utf-8"))
                except json.JSONDecodeError:
                    pass
            time.sleep(0.01)
        raise RuntimeError(f"Helper did not write {path.name} before deadline")

    def helper_process_handles(self, path):
        kernel = build.windows_api()
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        handles = []
        for pid in self.await_test_json(path):
            handle = kernel.OpenProcess(0x00100000, False, pid)
            if not handle:
                raise RuntimeError(f"Could not open private helper PID {pid}")
            self.addCleanup(kernel.CloseHandle, handle)
            handles.append(handle)
        return handles

    def helper_tree_command(self, path):
        return [sys.executable, "-c", (
            "import json,os,subprocess,sys,time; from pathlib import Path; "
            "child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); "
            f"Path({str(path)!r}).write_text(json.dumps([os.getpid(),child.pid]),encoding='utf-8'); "
            "time.sleep(60)"
        )]

    @unittest.skipUnless(os.name == "nt", "Windows focused-slot admission")
    def test_second_focused_slot_runs_beside_legacy_first_mutex_owner(self):
        self.control["maximum_focused_cargo_builds"] = 2
        self.save()
        name = "Local\\RustFalloutSecondSlotTest-" + str(uuid.uuid4())
        self.hold_test_mutex(name)
        output = self.root / "second-slot.json"
        command = [sys.executable, "-c", "from pathlib import Path; import sys; Path(sys.argv[1]).write_text('ran'); sys.exit(17)", str(output)]
        with patch.object(build, "MUTEX_NAMES", {"focused": name, "heavy": "unused"}), patch.object(build, "POLL_SECONDS", 0.02):
            self.assertEqual(build.run(self.lane, "test-session", "focused", command), 17)
        self.assertEqual(output.read_text(), "ran")

    @unittest.skipUnless(os.name == "nt", "Windows focused-slot admission")
    def test_third_focused_command_is_busy_without_launching(self):
        self.control["maximum_focused_cargo_builds"] = 2
        self.save()
        name = "Local\\RustFalloutThirdSlotTest-" + str(uuid.uuid4())
        self.hold_test_mutex(name)
        self.hold_test_mutex(name + "-2")
        with patch.object(build, "MUTEX_NAMES", {"focused": name, "heavy": "unused"}), patch.object(build, "WindowsChild", wraps=build.WindowsChild) as child:
            self.assertEqual(build.run(self.lane, "test-session", "focused", [sys.executable, "-c", "raise SystemExit(99)"]), build.BUSY)
            child.assert_not_called()

    @unittest.skipUnless(os.name == "nt", "Windows focused-slot admission")
    def test_default_capacity_one_refuses_free_second_mutex(self):
        name = "Local\\RustFalloutSerialSlotTest-" + str(uuid.uuid4())
        self.hold_test_mutex(name)
        with build.WindowsMutex(name + "-2") as second:
            self.assertTrue(second.acquire())
        with patch.object(build, "MUTEX_NAMES", {"focused": name, "heavy": "unused"}), patch.object(build, "WindowsChild", wraps=build.WindowsChild) as child:
            self.assertEqual(build.run(self.lane, "test-session", "focused", [sys.executable, "-c", "raise SystemExit(99)"]), build.BUSY)
            child.assert_not_called()

    @unittest.skipUnless(os.name == "nt", "Windows focused-slot revocation")
    def test_reduced_budget_stops_only_second_slot_tree_and_releases_mutex(self):
        self.control["maximum_focused_cargo_builds"] = 2
        self.save()
        name = "Local\\RustFalloutBudgetTest-" + str(uuid.uuid4())
        first_receipt, second_receipt = self.root / "first-tree.json", self.root / "second-tree.json"
        first_owner = self.hold_test_mutex(name, self.helper_tree_command(first_receipt))
        first_handles = self.helper_process_handles(first_receipt)
        second_handles, failures = [], []

        def reduce_budget():
            try:
                second_handles.extend(self.helper_process_handles(second_receipt))
                self.control["maximum_focused_cargo_builds"] = 1
                self.save()
            except BaseException as error:
                failures.append(error)
                self.control["stop_requested"] = True
                self.save()

        reducer = threading.Thread(target=reduce_budget)
        reducer.start()
        try:
            with patch.object(build, "MUTEX_NAMES", {"focused": name, "heavy": "unused"}), patch.object(build, "POLL_SECONDS", 0.02):
                with self.assertRaisesRegex(RuntimeError, "outside the current build budget"):
                    build.run(self.lane, "test-session", "focused", self.helper_tree_command(second_receipt))
        finally:
            reducer.join(15)
        self.assertFalse(reducer.is_alive())
        self.assertEqual(failures, [])
        self.assertEqual(len(second_handles), 2)
        kernel = build.windows_api()
        for handle in second_handles:
            self.assertEqual(kernel.WaitForSingleObject(handle, 5000), build.WAIT_OBJECT_0)
        for handle in first_handles:
            self.assertEqual(kernel.WaitForSingleObject(handle, 0), build.WAIT_TIMEOUT)
        self.assertTrue(first_owner.is_alive())
        with build.WindowsMutex(name) as first, build.WindowsMutex(name + "-2") as second:
            self.assertFalse(first.acquire())
            self.assertTrue(second.acquire())

    @unittest.skipUnless(os.name == "nt", "Windows focused-slot revocation")
    def test_reduced_budget_keeps_running_first_slot_authorized(self):
        self.control["maximum_focused_cargo_builds"] = 2
        self.save()
        name = "Local\\RustFalloutFirstBudgetTest-" + str(uuid.uuid4())
        receipt = self.root / "first-budget.json"
        failures = []

        def reduce_budget():
            try:
                self.await_test_json(receipt)
                self.control["maximum_focused_cargo_builds"] = 1
                self.save()
            except BaseException as error:
                failures.append(error)
                self.control["stop_requested"] = True
                self.save()

        reducer = threading.Thread(target=reduce_budget)
        reducer.start()
        command = [sys.executable, "-c", "import json,sys,time; from pathlib import Path; Path(sys.argv[1]).write_text(json.dumps('started')); time.sleep(0.3); sys.exit(37)", str(receipt)]
        try:
            with patch.object(build, "MUTEX_NAMES", {"focused": name, "heavy": "unused"}), patch.object(build, "POLL_SECONDS", 0.02):
                self.assertEqual(build.run(self.lane, "test-session", "focused", command), 37)
        finally:
            reducer.join(15)
        self.assertFalse(reducer.is_alive())
        self.assertEqual(failures, [])

    @unittest.skipUnless(os.name == "nt", "Windows job cancellation")
    def test_real_stop_cancels_child_and_grandchild(self):
        receipt = self.root / "stop-descendants.json"
        command = [sys.executable, "-c", (
            "import json,os,subprocess,sys,time; from pathlib import Path; "
            "child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); "
            f"Path({str(receipt)!r}).write_text(json.dumps([os.getpid(),child.pid]),encoding='utf-8'); "
            "time.sleep(60)"
        )]
        kernel = build.windows_api()
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        handles, failures = [], []
        def stop_after_start():
            try:
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    if receipt.exists():
                        try:
                            pids = json.loads(receipt.read_text(encoding="utf-8"))
                        except json.JSONDecodeError:
                            continue
                        for pid in pids:
                            handle = kernel.OpenProcess(0x00100000, False, pid)
                            if not handle:
                                raise RuntimeError(f"Could not open helper PID {pid}")
                            handles.append(handle)
                        return
                    time.sleep(0.01)
                raise RuntimeError("Child did not start before STOP test deadline")
            except Exception as error:
                failures.append(error)
            finally:
                self.control["stop_requested"] = True
                self.save()
        stopper = threading.Thread(target=stop_after_start)
        stopper.start()
        try:
            slots = {"focused": "Local\\RustFalloutStopTest-" + str(uuid.uuid4()), "heavy": "unused"}
            with patch.object(build, "MUTEX_NAMES", slots), patch.object(build, "POLL_SECONDS", 0.02):
                with self.assertRaisesRegex(RuntimeError, "stopped or inactive"):
                    build.run(self.lane, "test-session", "focused", command)
            stopper.join(5)
            self.assertFalse(stopper.is_alive())
            self.assertEqual(failures, [])
            self.assertEqual(len(handles), 2)
            for handle in handles:
                self.assertEqual(kernel.WaitForSingleObject(handle, 5000), build.WAIT_OBJECT_0)
        finally:
            stopper.join(15)
            for handle in handles:
                kernel.CloseHandle(handle)


class ReadTests(unittest.TestCase):
    def test_transient_open_race_retries(self):
        with patch.object(Path, "read_text", side_effect=[PermissionError(), '{"ok":true}']), patch.object(build.time, "sleep") as sleep:
            self.assertEqual(build.read_json(Path("unused")), {"ok": True})
            sleep.assert_called_once()

    def test_missing_file_and_malformed_json_remain_failures(self):
        with patch.object(Path, "read_text", side_effect=FileNotFoundError()) as read, patch.object(build.time, "sleep"):
            with self.assertRaises(FileNotFoundError):
                build.read_json(Path("unused"))
            self.assertEqual(read.call_count, build.READ_ATTEMPTS)
        with patch.object(Path, "read_text", return_value="{") as read:
            with self.assertRaises(json.JSONDecodeError):
                build.read_json(Path("unused"))
            read.assert_called_once()


@unittest.skipUnless(os.name == "nt", "Windows job and mutex integration")
class WindowsLifetimeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="fallout-team-job-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.kernel = build.windows_api()
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE

    def await_file(self, path, process=None):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if path.exists():
                try:
                    return json.loads(path.read_text(encoding="utf-8"))
                except json.JSONDecodeError:
                    pass
            if process is not None and process.poll() is not None:
                self.fail(f"Helper exited before writing {path.name}: {process.poll()}")
            time.sleep(0.02)
        self.fail(f"Timed out waiting for {path.name}")

    def process_handle(self, pid):
        handle = self.kernel.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
        self.assertTrue(handle, f"Could not open helper PID {pid}")
        self.addCleanup(self.kernel.CloseHandle, handle)
        return handle

    def assert_stopped(self, handle):
        self.assertEqual(self.kernel.WaitForSingleObject(handle, 5000), build.WAIT_OBJECT_0)

    def tree_command(self, path):
        code = (
            "import json,os,subprocess,sys,time; from pathlib import Path; "
            "child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); "
            f"Path({str(path)!r}).write_text(json.dumps([os.getpid(),child.pid]),encoding='utf-8'); "
            "time.sleep(60)"
        )
        return [sys.executable, "-c", code]

    def test_mutex_rejects_other_thread_then_releases(self):
        name = "Local\\RustFalloutTest-" + str(uuid.uuid4())
        outcomes = []
        def try_slot():
            with build.WindowsMutex(name) as other:
                outcomes.append(other.acquire())
        with build.WindowsMutex(name) as first:
            self.assertTrue(first.acquire())
            thread = threading.Thread(target=try_slot)
            thread.start()
            thread.join(5)
            self.assertFalse(thread.is_alive())
        thread = threading.Thread(target=try_slot)
        thread.start()
        thread.join(5)
        self.assertEqual(outcomes, [False, True])

    def test_job_preserves_arguments_exit_status_and_cleans_descendants(self):
        output = self.root / "argument receipt.json"
        arguments = ["space value", 'quote"value', "literal $() and `backticks`", "unicode é"]
        code = "import json,sys; from pathlib import Path; Path(sys.argv[1]).write_text(json.dumps(sys.argv[2:]),encoding='utf-8'); sys.exit(37)"
        with build.WindowsChild([sys.executable, "-c", code, str(output), *arguments], self.root) as child:
            deadline = time.monotonic() + 10
            while child.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertEqual(child.poll(), 37)
        self.assertEqual(json.loads(output.read_text(encoding="utf-8")), arguments)

        receipt = self.root / "descendants.json"
        with build.WindowsChild(self.tree_command(receipt), self.root) as child:
            handles = [self.process_handle(pid) for pid in self.await_file(receipt, child)]
        for handle in handles:
            self.assert_stopped(handle)

    def test_captured_stdout_and_stderr_survive_missing_stdin(self):
        script = self.root / "output-wrapper.py"
        script.write_text(
            "import ctypes,runpy,sys,time\nfrom ctypes import wintypes\nfrom pathlib import Path\n"
            f"module=runpy.run_path({str(SOURCE)!r})\n"
            "kernel=module['windows_api']()\n"
            "kernel.SetStdHandle.argtypes=[wintypes.DWORD,wintypes.HANDLE]\n"
            "kernel.SetStdHandle.restype=wintypes.BOOL\n"
            "assert kernel.SetStdHandle((-10)&0xFFFFFFFF,None)\n"
            "command=[sys.executable,'-c',\"import sys; print('captured stdout'); print('captured stderr',file=sys.stderr); sys.exit(23)\"]\n"
            f"with module['WindowsChild'](command,Path({str(self.root)!r})) as child:\n"
            "    while child.poll() is None: time.sleep(0.01)\n"
            "    result=child.poll()\n"
            "sys.exit(result)\n",
            encoding="utf-8",
        )
        result = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual(result.stdout.strip(), "captured stdout")
        self.assertEqual(result.stderr.strip(), "captured stderr")

    def test_full_runner_preserves_captured_powershell_output(self):
        worktree = self.root / "checkout"
        worktree.mkdir()
        subprocess.run(["git", "init", "--quiet", "-b", "agents/test", str(worktree)], check=True, capture_output=True)
        team = self.root / "local" / "team-v3"
        (team / "leases").mkdir(parents=True)
        (self.root / "local" / "team").mkdir()
        identity = {"generation": "team-v3-test", "run_id": "test-run", "lane": "scripts"}
        control = {
            "generation": "team-v3-test", "run_id": "test-run", "mode": "active",
            "stop_requested": False, "focused_build_policy": "automatic_mutex",
            "workers": ["scripts"], "active_coordination_directory": str(team),
        }
        assignment = {
            **identity, "state": "active", "implementation_authorized": True,
            "worktree": str(worktree), "branch": "agents/test",
            "cargo_target_directory": str(worktree / "target"), "allowed_resource_slots": ["focused"],
        }
        lease = {**identity, "session_uuid": "test-session", "worktree": str(worktree), "branch": "agents/test"}
        status = {**identity, "session_uuid": "test-session", "state": "active"}
        for path, value in (
            (self.root / "local" / "team" / "control.json", control),
            (team / "scripts.assignment.json", assignment),
            (team / "leases" / "scripts.json", lease),
            (team / "scripts.status.json", status),
        ):
            path.write_text(json.dumps(value), encoding="utf-8-sig")
        command = ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", "[Console]::WriteLine('powershell stdout'); [Console]::Error.WriteLine('powershell stderr'); exit 29"]
        script = self.root / "runner.py"
        script.write_text(
            "import os,runpy,sys\nfrom pathlib import Path\n"
            f"module=runpy.run_path({str(SOURCE)!r})\n"
            "scope=module['run'].__globals__\n"
            f"scope['ROOT']=Path({str(self.root)!r})\n"
            f"scope['MUTEX_NAMES']={{'focused':{('Local' + chr(92) + 'RustFalloutOutputTest-' + str(uuid.uuid4()))!r},'heavy':'unused'}}\n"
            f"os.chdir({str(worktree)!r})\n"
            f"os.environ['CARGO_TARGET_DIR']={str(worktree / 'target')!r}\n"
            f"sys.exit(module['run']('scripts','test-session','focused',{command!r}))\n",
            encoding="utf-8",
        )
        result = subprocess.run([sys.executable, str(script)], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 29, result.stderr)
        self.assertIn("focused slot acquired", result.stdout)
        self.assertIn("powershell stdout", result.stdout)
        self.assertEqual(result.stderr.strip(), "powershell stderr")

    def test_killed_wrapper_kills_tree_and_releases_abandoned_mutex(self):
        receipt = self.root / "crash-descendants.json"
        mutex_name = "Local\\RustFalloutCrashTest-" + str(uuid.uuid4())
        script = self.root / "wrapper.py"
        script.write_text(
            "import runpy,time\nfrom pathlib import Path\n"
            f"module=runpy.run_path({str(SOURCE)!r})\n"
            f"with module['WindowsMutex']({mutex_name!r}) as mutex:\n"
            "    assert mutex.acquire()\n"
            f"    with module['WindowsChild']({self.tree_command(receipt)!r},Path({str(self.root)!r})):\n"
            "        time.sleep(60)\n",
            encoding="utf-8",
        )
        wrapper = subprocess.Popen([sys.executable, str(script)], cwd=self.root)
        try:
            handles = [self.process_handle(pid) for pid in self.await_file(receipt, wrapper)]
            with build.WindowsMutex(mutex_name) as contender:
                self.assertFalse(contender.acquire())
                wrapper.kill()
                wrapper.wait(timeout=5)
                self.assertTrue(contender.acquire(5000))
            for handle in handles:
                self.assert_stopped(handle)
        finally:
            if wrapper.poll() is None:
                wrapper.kill()
            wrapper.wait(timeout=5)

    def test_start_failure_does_not_poison_slot(self):
        name = "Local\\RustFalloutFailureTest-" + str(uuid.uuid4())
        with self.assertRaises(OSError):
            with build.WindowsMutex(name) as mutex:
                self.assertTrue(mutex.acquire())
                build.WindowsChild([str(self.root / "does-not-exist.exe")], self.root)
        with build.WindowsMutex(name) as mutex:
            self.assertTrue(mutex.acquire())


if __name__ == "__main__":
    unittest.main()
