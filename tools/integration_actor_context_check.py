"""Exercise explicit native migration and the real actor context consumer.

The legacy envelope is authored transport for a preserved project fixture. It is
not an original game save. Only the Rust migrator changes the snapshot schema.
Run this under the coordinator's existing team-build admission wrapper.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys


PACKAGE_RECEIPT = "9e5b34b1ac14940a674d9a33833d55221828d992a66d059f5e73f255adc11bad"
ACTOR_COMMIT = "b433fb6e627fa33bdc20e4689bc9ede233d5954f"
REFERENCE_COMMIT = "55935d3a963559c777fda6699935c689555f21db"
MAX_STATE = 64 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_json(path):
    return json.loads(path.read_bytes())


def record(path):
    data = path.read_bytes()
    return {"path": str(path), "bytes": len(data), "sha256": digest(data)}


def write_new(path, data):
    with path.open("xb") as stream:
        stream.write(data)


def write_json(path, value):
    write_new(path, (json.dumps(value, indent=2) + "\n").encode())


def authored_envelope(body):
    """Wrap exact schema-3 bytes; generation 1 belongs only to this transport."""
    require(len(body) <= MAX_STATE, "legacy snapshot byte limit")
    state = json.loads(body)
    require(state["schema_version"] == 3, "expected preserved schema-3 snapshot")
    require("reference_states" not in state, "legacy fixture contains new component")
    meta = (struct.pack("<IIQQ", 1, 3, 1, state["clocks"]["tick"])
            + bytes.fromhex(state["catalogue_sha256"])
            + struct.pack("<Q", len(body)) + bytes(state["campaign"])
            + struct.pack("<Q", state["state_revision"]))
    require(len(meta) == 88, "META extent")
    wire = b"FRSAVE01" + struct.pack("<HHI", 1, 0, 2)
    for tag, payload in [(b"META", meta), (b"STAT", body)]:
        wire += tag + struct.pack("<IQ", 1, len(payload))
        wire += hashlib.sha256(payload).digest() + payload
    return wire + hashlib.sha256(wire).digest()


def extract_state(wire, schema):
    """Check the pinned envelope independently before exporting its exact STAT."""
    require(232 <= len(wire) <= MAX_STATE + 232, "container byte limit")
    require(wire[:16] == b"FRSAVE01" + struct.pack("<HHI", 1, 0, 2), "container header")
    require(hashlib.sha256(wire[:-32]).digest() == wire[-32:], "container digest")
    offset, chunks = 16, {}
    for tag in (b"META", b"STAT"):
        require(offset + 48 <= len(wire) - 32, "chunk header extent")
        actual, version, size = struct.unpack_from("<4sIQ", wire, offset)
        require(actual == tag and version == 1, "chunk identity")
        expected = wire[offset + 16:offset + 48]
        offset += 48
        require(size <= len(wire) - 32 - offset, "chunk payload extent")
        payload = wire[offset:offset + size]
        require(hashlib.sha256(payload).digest() == expected, "chunk digest")
        chunks[tag] = payload
        offset += size
    require(offset == len(wire) - 32, "container trailing bytes")
    meta, body = chunks[b"META"], chunks[b"STAT"]
    require(len(meta) == 88 and len(wire) == len(body) + 232, "container layout")
    state = json.loads(body)
    require(struct.unpack_from("<II", meta) == (1, schema), "META schema/profile")
    require(state["schema_version"] == schema and state["profile"] == "nv-original", "STAT schema/profile")
    require(struct.unpack_from("<Q", meta, 8)[0] > 0, "publication generation")
    require(struct.unpack_from("<Q", meta, 16)[0] == state["clocks"]["tick"], "META clock")
    require(meta[24:56].hex() == state["catalogue_sha256"], "META cohort")
    require(struct.unpack_from("<Q", meta, 56)[0] == len(body), "META STAT length")
    require(meta[64:80] == bytes(state["campaign"]), "META campaign")
    require(struct.unpack_from("<Q", meta, 80)[0] == state["state_revision"], "META revision")
    return body


def check_active(path):
    control = read_json(path)
    require(control.get("mode") == "active" and not control.get("stop_requested"), "team is stopped or inactive")
    return control["run_id"]


def run(args):
    run_id = check_active(args.control)
    package, binary, descriptor = (p.resolve(strict=True) for p in (args.package, args.binary, args.condition_executable))
    package_receipt = package / "handoff-01.json"
    require(digest(package_receipt.read_bytes()) == PACKAGE_RECEIPT, "ACT02 handoff identity")
    handoff = read_json(package_receipt)
    source = package / "export-03" / "authored-packages"
    snapshot, expected_path = source / "snapshot.json", source / "expected.json"
    protected = [package_receipt, snapshot, expected_path, source / "order.json",
                 source / "Data" / "FalloutNV.esm", package / "faithful-03.json",
                 package / "absent-subject-03.json", descriptor, binary]
    before = [record(p) for p in protected]
    require(before[-1]["sha256"] == args.binary_sha256, "frozen current binary identity")
    require(before[-2]["sha256"] == handoff["source_pins"]["descriptor_executable_sha256"], "pinned descriptor identity")
    proof = handoff["validation"]["headless_consumer"]
    known = {Path(row["path"]).resolve(): row for row in proof["input_receipts"] + proof["evidence"]}
    for item in before[1:-2]:
        require(item["sha256"] == known[Path(item["path"])]["sha256"], "preserved input identity: " + item["path"])
    output = args.output.resolve()
    require(not output.exists(), "proof output must be fresh")
    require(not output.is_relative_to(package), "proof output overlaps preserved package")
    require(not output.is_relative_to(descriptor.parent), "proof output overlaps descriptor installation")
    output.mkdir(parents=True)
    inputs, reports = output / "inputs", output / "reports"
    inputs.mkdir()
    reports.mkdir()
    receipt = {"schema_version": 1, "run_id": run_id, "candidate_revision": args.candidate,
               "dependencies": {"actor_original": ACTOR_COMMIT, "reference_original": REFERENCE_COMMIT},
               "tool": record(Path(__file__).resolve()), "inputs_before": before, "commands": [],
               "scope": "Authored project snapshot transport; supported native migration and cold actor observations",
               "original_game_save": False, "retail_parity_accepted": False, "result": "running"}

    def invoke(name, arguments, expected_exit=0, reason=None):
        require(check_active(args.control) == run_id, "team run changed")
        report = reports / (name + ".json")
        command = [str(binary), *map(str, arguments), "--output", str(report)]
        result = subprocess.run(command, capture_output=True, timeout=120)
        write_new(reports / (name + ".stdout"), result.stdout)
        write_new(reports / (name + ".stderr"), result.stderr)
        receipt["commands"].append({"name": name, "command": command, "exit_code": result.returncode})
        require(result.returncode == expected_exit, name + ": unexpected exit")
        if expected_exit:
            require(not report.exists(), name + ": refusal published a report")
            if reason:
                require(any(word in result.stderr.decode(errors="replace").lower() for word in reason), name + ": unexpected refusal")
            return None
        return read_json(report)

    try:
        original = snapshot.read_bytes()
        envelope = authored_envelope(original)
        require(extract_state(envelope, 3) == original, "authored transport altered legacy STAT")
        legacy = inputs / "authored-schema3.frsv"
        write_new(legacy, envelope)
        repository = output / "migrated-repository"
        migration = invoke("migration", ["native-migrate-v3", "--install", source, "--load-order", source / "order.json",
                                        "--file", legacy, "--new-repository", repository])
        require(migration["source_state_schema"] == 3 and migration["target_state_schema"] == 4, "migration schema receipt")
        require(migration["source_bound_restore"] and migration["canonical_state_round_trip_equal"], "migration canonical validation")
        require(migration["source_metadata"]["container_sha256"] == digest(envelope), "migration input identity")
        body = extract_state((repository / "current.frsv").read_bytes(), 4)
        require(digest(body) == migration["snapshot_sha256"], "migration output identity")
        old, new = json.loads(original), json.loads(body)
        require(set(new) == set(old) | {"reference_states"}, "migration changed canonical fields")
        require(new["reference_states"] == [], "migration invented reference state")
        require(all(new[key] == value for key, value in old.items() if key != "schema_version"), "migration changed prior canonical state")
        migrated = inputs / "migrated-snapshot.json"
        write_new(migrated, body)

        def actor(name, current=migrated, subject="1", actor_root="FalloutNV.esm:100", engineering=True, **checks):
            command = ["actor-package-context", "--install", source, "--load-order", source / "order.json",
                       "--native-snapshot", current, "--actor-root", actor_root, "--condition-executable", descriptor]
            if subject is not None:
                command += ["--explicit-subject", subject]
            if engineering:
                command += ["--engineering-observation"]
            return invoke(name, command, **checks)

        actual = actor("engineering")
        expected = read_json(expected_path)
        require(actual["observation"] == expected, "complete preserved authored observation differs")
        require(actual["snapshot_input"]["sha256"] == digest(body), "actor snapshot identity")
        require(actual["descriptor_receipt"]["source_sha256"] == before[-2]["sha256"], "actor descriptor identity")
        require(not actual["state_changed"] and not actual["retail_parity_accepted"], "actor claimed unsupported authority")
        require((len(expected["packages"]), expected["condition_requests"], expected["query_contributions"]) == (7, 5, 4), "authored case scope")
        faithful = actor("faithful", engineering=False)
        absent = actor("absent-subject", subject=None)
        require(faithful["observation"] == read_json(package / "faithful-03.json")["observation"], "faithful refusal differs")
        require(absent["observation"] == read_json(package / "absent-subject-03.json")["observation"], "absent context outcome differs")
        actor("missing-reference", subject="999", expected_exit=1, reason=["reference"])
        actor("missing-actor", actor_root="FalloutNV.esm:888", expected_exit=1, reason=["actor winner"])
        actor("strict-legacy-refusal", current=snapshot, expected_exit=1, reason=["schema", "reference_states"])
        stale = inputs / "stale-source-snapshot.json"
        altered = dict(new)
        altered["catalogue_sha256"] = "0" * 64
        write_json(stale, altered)
        actor("stale-source", current=stale, expected_exit=1, reason=["definition", "source", "catalogue"])
        receipt["result"] = "passed"
        receipt["observed"] = {"packages": 7, "conditions": 5, "contributions": 4,
                               "complete_expected_equal": True, "legacy_fields_preserved": True,
                               "reference_state_unavailable": True}
    except BaseException as error:
        receipt["result"] = "failed"
        receipt["error"] = str(error)
        raise
    finally:
        after = [record(p) for p in protected]
        receipt["inputs_after"] = after
        receipt["protected_inputs_unchanged"] = before == after
        if before != after:
            receipt["result"] = "failed"
            receipt["error"] = "protected input identity changed"
        receipt["evidence"] = [record(p) for p in sorted(output.rglob("*")) if p.is_file()]
        write_json(output / "receipt.json", receipt)
        require(before == after, "protected input identity changed")
    print(json.dumps({"result": receipt["result"], "receipt": str(output / "receipt.json")}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--condition-executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--control", type=Path, default=Path(r"G:\Rust-Fallout\local\team\control.json"))
    run(parser.parse_args())


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
