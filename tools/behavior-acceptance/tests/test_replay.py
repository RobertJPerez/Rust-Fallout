import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import replay


SOURCE_REVISION = "d622128fb3fca1c67a1285dec85fd48c7526031f"
QUEST = {"profile": "nv_original", "origin_plugin": "falloutnv.esm", "local_id": 768}
PLAYER = {"kind": "live", "id": "1"}
CONTAINER = {"kind": "live", "id": "90"}


def _form(local_id):
    return {"profile": "nv_original", "origin_plugin": "falloutnv.esm", "local_id": local_id}


def fixture_timeline():
    return [
        {
            "ordinal": 1,
            "tick": 1,
            "kind": "input",
            "sequence": 1,
            "action": "activate",
            "source": {"kind": "synthetic", "device": "fixture", "control": "activate", "device_id": "fixture-1"},
            "context": {"name": "gameplay", "focused": True, "scene_epoch": 4, "expected_revision": 1},
            "request_id": "activate-1",
            "outcome": "accepted",
        },
        {
            "ordinal": 2,
            "tick": 1,
            "kind": "event",
            "event_sequence": 9,
            "owner": {"kind": "quest", "key": QUEST},
            "definition": QUEST,
            "event_id": "on_activate:0x20",
            "status": "dispatched",
            "effects": [
                {"kind": "inventory_transfer", "lot_id": "lot-7", "owner_before": CONTAINER, "owner_after": PLAYER},
                {"kind": "quest_fact", "quest": QUEST, "field": "stage", "value": 20},
            ],
            "error": None,
        },
        {
            "ordinal": 3,
            "tick": 1,
            "kind": "acknowledgment",
            "event_sequence": 9,
            "instance_id": "31",
            "definition": QUEST,
            "before_revision": 1,
            "after_revision": 2,
            "status": "acknowledged",
            "receipt_sha256": "a" * 64,
        },
        {
            "ordinal": 4,
            "tick": 1,
            "kind": "state",
            "observation_id": "after-activation",
            "revision": 2,
            "visible_revision": 2,
            "snapshot_sha256": "b" * 64,
            "inventory": [
                {"lot_id": "lot-7", "owner": PLAYER, "base": _form(1280), "count": 1, "facts": {"condition_bits": "3ff0000000000000"}}
            ],
            "quest_state": [{"quest": QUEST, "field": "stage", "value": 20}],
            "pending_events": [],
            "acknowledged_events": [9],
        },
    ]


def fixture_capture(timeline=None):
    timeline = copy.deepcopy(timeline if timeline is not None else fixture_timeline())
    return {
        "format": replay.CAPTURE_FORMAT,
        "schema_version": 1,
        "scenario_id": "fixture-container-activation",
        "evidence_class": "synthetic_fixture",
        "complete": True,
        "skipped_steps": 0,
        "dropped_records": 0,
        "provenance": {
            "source_revision": SOURCE_REVISION,
            "binary_sha256": None,
            "profile_id": "synthetic-nv",
            "profile_fingerprint_sha256": None,
            "content_fingerprint_sha256": None,
            "process": None,
        },
        "trace_sha256": replay._sha256(timeline),
        "timeline": timeline,
    }


def fixture_expectation(timeline=None):
    expected = copy.deepcopy(timeline if timeline is not None else fixture_timeline())
    return {
        "format": replay.EXPECTATION_FORMAT,
        "schema_version": 1,
        "scenario_id": "fixture-container-activation",
        "evidence_class": "synthetic_fixture",
        "oracle": {"kind": "independent_fixture", "id": "acceptance-fixture-01", "sha256": replay._sha256(expected)},
        "provenance_pins": {
            "source_revision": SOURCE_REVISION,
            "binary_sha256": None,
            "profile_id": "synthetic-nv",
            "profile_fingerprint_sha256": None,
            "content_fingerprint_sha256": None,
        },
        "minimum_counts": {"input": 1, "state": 1, "event": 1, "acknowledgment": 1, "save": 0},
        "expected_timeline": expected,
    }


def engineering_envelope_fixture():
    # Synthetic values exercise the engineering contract; no process was run.
    capture = fixture_capture()
    expectation = fixture_expectation()
    capture["evidence_class"] = "engineering_capture"
    capture["provenance"].update(
        {
            "binary_sha256": "c" * 64,
            "profile_id": "nv-original-profile",
            "profile_fingerprint_sha256": "d" * 64,
            "content_fingerprint_sha256": "e" * 64,
            "process": {
                "pid": 1732,
                "started_utc": "2026-10-04T14:00:00Z",
                "ended_utc": "2026-10-04T14:00:01Z",
                "exit_code": 0,
            },
        }
    )
    capture["timeline"][0]["source"].update(
        kind="physical", device="gamepad", control="activate", device_id="pad-1"
    )
    resign_timeline(capture)

    expectation["evidence_class"] = "engineering_capture"
    expectation["oracle"].update(
        kind="frozen_engine_capture", id="unit-test-engineering-envelope"
    )
    expectation["expected_timeline"] = copy.deepcopy(capture["timeline"])
    expectation["provenance_pins"].update(
        {
            "binary_sha256": "c" * 64,
            "profile_id": "nv-original-profile",
            "profile_fingerprint_sha256": "d" * 64,
            "content_fingerprint_sha256": "e" * 64,
        }
    )
    expectation["oracle"]["sha256"] = replay._sha256(expectation["expected_timeline"])
    return capture, expectation


def resign_timeline(capture):
    capture["trace_sha256"] = replay._sha256(capture["timeline"])


class ReplayTests(unittest.TestCase):
    def test_exact_independent_trace_passes_as_fixture_only(self):
        result = replay.compare(fixture_capture(), fixture_expectation())
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["classification"], "synthetic_fixture_validation")
        self.assertIs(result["retail_pass"], False)

    def _assert_mutation_fails(self, mutate, expected_path):
        capture = fixture_capture()
        mutate(capture)
        resign_timeline(capture)
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["status"], "failed")
        self.assertIs(result["retail_pass"], False)
        self.assertTrue(any(m.get("path", "").startswith(expected_path) for m in result["mismatches"]))

    def test_wrong_inventory_lot_or_quantity_fails(self):
        self._assert_mutation_fails(
            lambda c: c["timeline"][3]["inventory"][0].update(count=2),
            "/timeline/3/inventory/0/count",
        )

    def test_wrong_quest_observation_fails(self):
        self._assert_mutation_fails(
            lambda c: c["timeline"][3]["quest_state"][0].update(value=21),
            "/timeline/3/quest_state/0/value",
        )

    def test_missing_or_wrong_acknowledgement_fails(self):
        capture = fixture_capture()
        capture["timeline"].pop(2)
        for ordinal, row in enumerate(capture["timeline"], start=1):
            row["ordinal"] = ordinal
        resign_timeline(capture)
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["status"], "failed")
        self.assertTrue(any(m.get("path", "").startswith("/timeline") for m in result["mismatches"]))

        self._assert_mutation_fails(
            lambda c: c["timeline"][2].update(event_sequence=10),
            "/timeline/2/event_sequence",
        )

    def test_reordered_timeline_and_input_actions_fail(self):
        self._assert_mutation_fails(
            lambda c: c["timeline"].reverse(),
            "/",
        )
        self._assert_mutation_fails(
            lambda c: c["timeline"][0].update(action="wait"),
            "/timeline/0/action",
        )

    def test_missing_duplicate_and_reordered_effects_fail(self):
        self._assert_mutation_fails(
            lambda c: c["timeline"][1]["effects"].pop(),
            "/timeline/1/effects",
        )
        self._assert_mutation_fails(
            lambda c: c["timeline"][1]["effects"].append(
                copy.deepcopy(c["timeline"][1]["effects"][0])
            ),
            "/timeline/1/effects",
        )
        self._assert_mutation_fails(
            lambda c: c["timeline"][1]["effects"].reverse(),
            "/timeline/1/effects",
        )

    def test_wrong_event_owner_and_unsupported_success_fail(self):
        self._assert_mutation_fails(
            lambda c: c["timeline"][1]["owner"]["key"].update(local_id=769),
            "/timeline/1/owner/key/local_id",
        )

        capture = fixture_capture()
        capture["timeline"][1]["status"] = "success"
        resign_timeline(capture)
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["classification"], "invalid_receipt")

        capture = fixture_capture()
        expectation = fixture_expectation()
        expected_event = expectation["expected_timeline"][1]
        expected_event["status"] = "unsupported"
        expected_event["error"] = "unsupported opcode"
        expectation["oracle"]["sha256"] = replay._sha256(expectation["expected_timeline"])
        result = replay.compare(capture, expectation)
        self.assertEqual(result["status"], "failed")
        self.assertTrue(
            any(m.get("path") == "/timeline/1/status" for m in result["mismatches"])
        )

    def test_missing_quest_state_does_not_default_to_success(self):
        capture = fixture_capture()
        capture["timeline"][3]["quest_state"] = []
        resign_timeline(capture)
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["status"], "failed")
        self.assertTrue(any("quest_state" in m.get("path", "") for m in result["mismatches"]))

    def test_stale_revision_or_profile_pin_fails(self):
        capture = fixture_capture()
        capture["provenance"]["source_revision"] = "1" * 40
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["status"], "failed")
        self.assertTrue(any(m.get("path") == "/provenance/source_revision" for m in result["mismatches"]))

    def test_engineering_capture_requires_and_matches_all_fingerprints(self):
        capture, expectation = engineering_envelope_fixture()
        result = replay.compare(capture, expectation)
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["classification"], "engineering_replay_validation")
        self.assertIs(result["retail_pass"], False)

        for field in ("binary_sha256", "profile_fingerprint_sha256", "content_fingerprint_sha256"):
            capture, expectation = engineering_envelope_fixture()
            capture["provenance"][field] = "0" * 64
            result = replay.compare(capture, expectation)
            self.assertEqual(result["status"], "failed")
            self.assertTrue(
                any(m.get("path") == f"/provenance/{field}" for m in result["mismatches"])
            )

        capture, expectation = engineering_envelope_fixture()
        capture["provenance"]["content_fingerprint_sha256"] = None
        result = replay.compare(capture, expectation)
        self.assertEqual(result["classification"], "invalid_receipt")

        capture, expectation = engineering_envelope_fixture()
        expectation["provenance_pins"]["content_fingerprint_sha256"] = None
        result = replay.compare(capture, expectation)
        self.assertEqual(result["classification"], "invalid_receipt")

        capture, expectation = engineering_envelope_fixture()
        capture["provenance"]["process"] = None
        result = replay.compare(capture, expectation)
        self.assertEqual(result["classification"], "invalid_receipt")

    def test_damaged_trace_and_truncated_capture_are_rejected(self):
        capture = fixture_capture()
        capture["timeline"][0]["action"] = "unrecorded-action"
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["classification"], "invalid_receipt")

        with tempfile.TemporaryDirectory() as directory:
            damaged = Path(directory) / "truncated-capture.json"
            damaged.write_bytes(b'{"format":"rust-fallout.behavior-capture",')
            with self.assertRaisesRegex(replay.ReceiptError, "not valid UTF-8 JSON"):
                replay.load_json(damaged)

    def test_unknown_retail_claim_class_is_refused_by_v1(self):
        capture = fixture_capture()
        capture["evidence_class"] = "retail_observation"
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["classification"], "invalid_receipt")
        self.assertIs(result["retail_pass"], False)

    def test_incomplete_or_skipped_capture_fails(self):
        capture = fixture_capture()
        capture["skipped_steps"] = 1
        result = replay.compare(capture, fixture_expectation())
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["classification"], "invalid_receipt")

    def test_collector_packs_ordered_jsonl_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = {
                "scenario_id": "fixture-container-activation",
                "evidence_class": "synthetic_fixture",
                "complete": True,
                "skipped_steps": 0,
                "dropped_records": 0,
                "provenance": fixture_capture()["provenance"],
            }
            manifest_path = root / "manifest.json"
            timeline_path = root / "timeline.jsonl"
            output_path = root / "capture.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            timeline_path.write_text("\n".join(json.dumps(r) for r in fixture_timeline()) + "\n", encoding="utf-8")
            result = replay.collect(manifest_path, timeline_path, output_path)
            self.assertEqual(result["status"], "captured")
            capture = replay.load_json(output_path)
            self.assertEqual(replay.compare(capture, fixture_expectation())["status"], "passed")
            with self.assertRaises(replay.ReceiptError):
                replay.collect(manifest_path, timeline_path, output_path)

    def test_collector_rejects_gaps_and_duplicate_json_properties(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = {
                "scenario_id": "fixture-container-activation",
                "evidence_class": "synthetic_fixture",
                "complete": True,
                "skipped_steps": 0,
                "dropped_records": 0,
                "provenance": fixture_capture()["provenance"],
            }
            manifest_path = root / "manifest.json"
            timeline_path = root / "timeline.jsonl"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            broken = fixture_timeline()
            broken[1]["ordinal"] = 4
            timeline_path.write_text("\n".join(json.dumps(r) for r in broken), encoding="utf-8")
            with self.assertRaisesRegex(replay.ReceiptError, "contiguous"):
                replay.collect(manifest_path, timeline_path, root / "gap.json")
            with self.assertRaisesRegex(replay.ReceiptError, "duplicate JSON property"):
                replay._decode_json(b'{"x":1,"x":2}', "duplicate-test")


if __name__ == "__main__":
    unittest.main()
