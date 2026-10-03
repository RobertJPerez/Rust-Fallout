"""Audit operand observations against native source tuples and a saved fixture.

This deliberately accepts only the engineering fixture: fragment instances,
unmapped registered references, and no explicit player binding. It checks our
storage contract; it is not an oracle for retail VM values or native arguments.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import struct

TUPLE = struct.Struct("<IBHBHBBIBIBIBBBI")


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def read(path, maximum=256 * 1024 * 1024):
    require(path.stat().st_size <= maximum, f"Input budget exceeded: {path}")
    return path.read_bytes()


def document(path):
    return json.loads(read(path))


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def key(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def snapshot(raw):
    require(len(raw) >= 48 and raw[:8] == b"FRSAVE01", "Invalid saved fixture")
    version, count = struct.unpack_from("<II", raw, 8)
    require(version == 1 and count == 2, "Unsupported saved fixture framing")
    require(sha(raw[:-32]) == raw[-32:].hex(), "Saved fixture container digest differs")
    chunks = {}
    offset = 16
    for _ in range(count):
        require(offset + 48 <= len(raw) - 32, "Truncated saved fixture header")
        tag, version, size = struct.unpack_from("<4sIQ", raw, offset)
        start = offset + 48
        require(start + size <= len(raw) - 32, "Truncated saved fixture body")
        body = raw[start:start + size]
        require(version == 1 and tag not in chunks, "Invalid saved fixture chunk")
        require(sha(body) == raw[offset + 16:offset + 48].hex(), "Saved fixture chunk digest differs")
        chunks[tag] = body
        offset = start + size
    require(offset == len(raw) - 32 and set(chunks) == {b"META", b"STAT"}, "Saved fixture coverage differs")
    return json.loads(chunks[b"STAT"]), sha(chunks[b"STAT"])


def encoded(binding):
    # The native reader publishes the same fixed 33-byte source association
    # format, independently produced from original compiled bytes and tables.
    optional = [binding[name] for name in (
        "context_reference", "target_value", "reference_field_decoded_offset",
        "local_declaration_decoded_offset", "local_type_byte")]
    context, target, reference, local, local_type = optional
    return TUPLE.pack(binding["scda_offset"], binding["role"], binding["index"],
                      context is not None, context or 0, binding["status"],
                      target is not None, target or 0, reference is not None,
                      reference or 0, local is not None, local or 0,
                      local_type is not None, local_type or 0,
                      binding["context_target_kind"], binding["context_target_value"])


def declarations(script):
    dynamic = {r["value"] for r in script["references"] if r["status"] == "dynamic_variable"}
    result = {}
    for source in script["declarations"]:
        index = source["index"]
        if index in result:
            continue
        kind = ("unverified_zero_index" if index == 0 else "reference" if index in dynamic
                else {0: "float", 1: "integer"}.get(source["type_byte"], "unsupported"))
        detail = {"kind": kind}
        if kind in {"unverified_zero_index", "unsupported"}:
            detail["type_byte"] = source["type_byte"]
        result[index] = {"index": index, "declaration_decoded_offset": source["decoded_offset"], "kind": detail}
    return result


def validate_projection(script, binding):
    """Tie the loaded storage projection to the independently read tuple.

    A matching source hash does not prove a projected field is correct. Check
    raw declaration/reference offsets and words before interpreting live values.
    """
    status = binding["status"]
    if status in {1, 3}:
        declaration = next((d for d in script["declarations"] if d["index"] == binding["target_value"]), None)
        require(declaration is not None and declaration["decoded_offset"] == binding["local_declaration_decoded_offset"]
                and declaration["type_byte"] == binding["local_type_byte"], "Loaded local declaration differs from native tuple")
        if status == 1:
            require(declaration["index"] == binding["index"], "Loaded local index differs from native tuple")
    context = binding["context_reference"]
    if context is not None or binding["role"] in {1, 3, 5, 6, 7, 9, 10, 12}:
        index = context if context is not None else binding["index"]
        source = next((r for r in script["references"] if r["index"] == index), None)
        if status == 6:
            require(source is None, "Native missing reference was supplied by loaded projection")
            return
        require(source is not None and source["decoded_offset"] == binding["reference_field_decoded_offset"],
                "Loaded reference field differs from native tuple")
        if context is not None:
            source_kind = {1: "SCRO", 2: "SCRV"}.get(binding["context_target_kind"])
            raw_value = binding["context_target_value"]
        else:
            source_kind = "SCRO" if status == 2 else "SCRV"
            raw_value = binding["target_value"]
        require(source["source_kind"] == source_kind and source["value"] == raw_value,
                "Loaded reference kind or target word differs from native tuple")
        if source_kind == "SCRO" and raw_value == 0:
            require(source["status"] == "null_form", "Loaded null reference status differs")
        if source_kind == "SCRV" and status < 5:
            require(source["status"] == "dynamic_variable", "Loaded SCRV storage status differs")


def storage(instance, schema, index, destination=False):
    declaration = schema.get(index)
    if declaration is None:
        return None, "missing_local"
    if declaration["kind"]["kind"] not in {"float", "integer", "reference"}:
        return None, "unsupported_local"
    if destination:
        return None, None
    # Preserve the saved payload, including NaN bits and distinct reference
    # identities. Do not turn the diagnostic into a numeric conversion.
    value = instance["values"].get(index)
    if value is None or value["kind"] == "uninitialized":
        return None, "uninitialized_local"
    return value, None


def reference(instance, script, schema, index, registered):
    rows = {r["index"]: r for r in script["references"]}
    source = rows.get(index)
    if source is None:
        return None, "unresolved_context_reference"
    status = source["status"]
    if status == "defined_form":
        require(source["form_key"] is not None, "Defined source reference lacks identity")
        return {"kind": "content", "key": source["form_key"]}, None
    if status == "null_form":
        return {"kind": "null"}, None
    if status == "dynamic_variable":
        value, error = storage(instance, schema, source["value"])
        if error:
            return None, error
        if value["kind"] != "reference":
            return None, "incompatible_local"
        payload = value["value"]
        if payload["kind"] == "live" and payload["id"] not in registered:
            return None, "missing_live_reference"
        return payload, None
    return None, "unresolved_context_reference"


def expected(instance, script, schema, binding, registered):
    role = binding["role"]
    if role in {3, 5, 9}:
        return {"status": "unresolved", "code": "unverified_global_value"}
    if role in {1, 6, 7, 10, 12}:
        value, error = reference(instance, script, schema, binding["index"], registered)
        if error:
            return {"status": "unresolved", "code": error}
        return {"status": "resolved", "access": "reference", "resolution": {"kind": "reference", "value": value}}
    require(role in {2, 4, 8, 11}, "Unsupported source role in fixture")
    context = binding["context_reference"]
    if context is not None:
        value, error = reference(instance, script, schema, context, registered)
        if error is None:
            if value["kind"] == "null":
                error = "null_context"
            elif value["kind"] == "live":
                # This fixture has no quest/placed owner instances. An authored
                # SCRI attachment cannot supply a missing live event list.
                error = "missing_live_event_list"
            else:
                source = next(r for r in script["references"] if r["index"] == context)
                require(source["target"] is not None, "Fixture lacks independent foreign target header")
                target = source["target"]
                if target["record_flags"] & 32:
                    error = "deleted_form"
                elif target["record_kind"] == "QUST":
                    error = "missing_live_event_list"
                elif target["record_kind"] in {"REFR", "ACHR", "ACRE", "PGRE", "PMIS", "PBEA"}:
                    error = "reference_not_registered"
                else:
                    error = "unsupported_form_kind"
        return {"status": "unresolved", "code": error}
    destination = role == 2
    value, error = storage(instance, schema, binding["index"], destination)
    if error is None and role == 11 and value["kind"] != "reference":
        error = "incompatible_local"
    if error:
        return {"status": "unresolved", "code": error}
    return {"status": "resolved", "access": "destination" if destination else "read",
            "resolution": {"kind": "local", "instance": instance["id"],
                           "declaration": schema[binding["index"]], "value": value}}


def compare(probe, frames, loaded, state, rust, native, bundle_sha, snapshot_sha):
    require(probe["explicit_player"] is None, "Fixture checker does not admit explicit player binding")
    require(all(i["owner"]["kind"] == "fragment" for i in state["instances"]), "Fixture has nonfragment owners")
    require(all(r["authored"] is None for r in state["references"]), "Fixture has authored live bindings")
    require(probe["snapshot_sha256"] == frames["snapshot_sha256"] == snapshot_sha, "Saved fixture identity differs")
    require(probe["catalogue_sha256"] == frames["catalogue_sha256"] == state["catalogue_sha256"], "Loaded catalogue differs")
    require(probe["sources"] == frames["sources"], "Pending source receipts differ")
    require(probe["sources"] == loaded["plugins"], "Loaded source cohort differs")
    source_receipts = sorted((p["source_name"].lower(), p["source_bytes"], p["source_sha256"]) for p in probe["sources"])
    require(source_receipts == sorted((p["source_name"].lower(), p["source_bytes"], p["source_sha256"]) for p in rust["plugins"]),
            "Native owning source cohort differs")
    require(probe["context_content"]["winning_headers_sha256"] == loaded["metadata"]["winning_definitions_sha256"],
            "Foreign owning header cohort differs")
    require(native["schema_version"] == 2 and native["bundle_sha256"] == rust["comparison_bundle"]["sha256"] == bundle_sha,
            "Native source bundle differs")
    require(native["executable_source_sha256"] == rust["executable_source_sha256"] == probe["executable_source_sha256"],
            "Native descriptor source differs")
    flat = [(plugin["source_name"].lower(), row) for plugin in rust["plugins"] for row in plugin["compiled_units"]]
    require(len(flat) == native["compiled_unit_count"] == len(native["compiled_units"]), "Native unit coverage differs")
    units = {}
    for (name, row), original in zip(flat, native["compiled_units"]):
        projected = {k: v for k, v in original.items() if k != "binding_tuples_hex"}
        require(row == projected, "Independent native unit projection differs")
        tuples = bytes.fromhex(original["binding_tuples_hex"])
        require(len(tuples) == TUPLE.size * row["counts"]["uses"] and sha(tuples) == row["binding_sha256"],
                "Independent native tuple length or digest differs")
        identity = (name, row["record_file_offset"], row["header_decoded_offset"])
        require(identity not in units, "Duplicate native owning unit")
        units[identity] = (row, tuples)
    scripts = {key(s["handle"]): s for s in loaded["scripts"]}
    require(len(scripts) == len(loaded["scripts"]), "Duplicate loaded source identity")
    instances = {i["id"]: dict(i, values={v["index"]: v["value"] for v in i["locals"]}) for i in state["instances"]}
    registered = {r["id"] for r in state["references"]}
    require(len(probe["events"]) == len(frames["events"]) == len(state["pending_events"]), "Pending journal coverage differs")
    counts, source_counts = Counter(), Counter()
    uses = probes = findings = attempted = retained = 0
    for row, frame, pending in zip(probe["events"], frames["events"], state["pending_events"]):
        retained += len(json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode())
        instance = instances[pending["instance"]]
        require(row["pending"] == frame["pending"] == pending, "Saved pending entry differs")
        require(row["definition"] == frame["definition"] == instance["definition"], "Saved definition differs")
        script = scripts[key(instance["definition"])]
        attempted += script["version"]["compiled_bytes"] or 0
        require(row["finding"] == frame["finding"], "Strict source finding differs")
        if frame["prepared"] is None:
            require(row["probe"] is None, "Rejected source was probed")
            findings += 1
            source_counts[row["finding"]["kind"]] += 1
            continue
        result, prepared = row["probe"], frame["prepared"]
        require(result is not None, "Prepared source was omitted")
        require(result["pending"] == pending and result["definition"] == instance["definition"], "Probe identity differs")
        require(result["campaign"] == state["campaign"] and result["state_revision"] == state["state_revision"]
                and result["catalogue_sha256"] == state["catalogue_sha256"], "Probe live state identity differs")
        start, end = result["begin_scda_offset"], result["end_scda_offset"]
        require(start == prepared["begin_scda_offset"] and end == prepared["end_scda_offset"], "Selected event window differs")
        version = script["version"]
        source, tuples = units[(version["source_plugin"].lower(), version["record_file_offset"], script["handle"]["key"]["header_decoded_offset"])]
        require(source["compiled_sha256"] == version["compiled_sha256"] and source["metadata_sha256"] == version["metadata_sha256"],
                "Winning source bytes or tables differ")
        require(result["full_definition_binding_sha256"] == prepared["binding_sha256"] == source["binding_sha256"], "Full binder identity differs")
        selected = b"".join(tuples[o:o + TUPLE.size] for o in range(0, len(tuples), TUPLE.size)
                            if start <= struct.unpack_from("<I", tuples, o)[0] < end)
        observed = b"".join(encoded(o["binding"]) for o in result["operands"])
        require(observed == selected, f"Selected native tuple coverage/order differs at event {pending['sequence']}")
        schema = declarations(script)
        for operand in result["operands"]:
            validate_projection(script, operand["binding"])
            wanted = expected(instance, script, schema, operand["binding"], registered)
            actual = {k: v for k, v in operand["outcome"].items() if k != "reason"}
            require(actual == wanted, f"Saved storage association differs at event {pending['sequence']} offset {operand['binding']['scda_offset']}")
            counts[actual["code"] if actual["status"] == "unresolved" else "resolved_" + actual["access"]] += 1
            uses += 1
        probes += 1
        source_counts["prepared_operand_probe"] += 1
    require(dict(counts) == probe["operand_counts"] and dict(source_counts) == probe["source_counts"], "Observed counters differ")
    require(probes == probe["prepared_probes"] and uses == probe["operand_uses"]
            and len(state["pending_events"]) == probe["pending_events_checked"] and attempted == probe["attempted_source_bytes"], "Reported coverage differs")
    require(retained == probe["retained_report_bytes"], "Retained report-byte counter differs")
    unresolved = sum(v for k, v in counts.items() if not k.startswith("resolved_"))
    require(unresolved == probe["unresolved_operands"], "Unresolved counter differs")
    require(probe["canonical_state_unchanged"] is True and probe["bytecode_executed"] is False
            and probe["native_readiness_accepted"] is False and probe["retail_parity_accepted"] is False
            and probe["accepted_scenarios"] == [], "Operand evidence exceeds its accepted scope")
    return {"schema_version": 1, "pending_events": len(state["pending_events"]), "prepared_probes": probes,
            "source_findings": findings, "selected_native_tuples_checked": uses,
            "saved_storage_outcomes_checked": uses, "unresolved_operands": unresolved,
            "operand_counts": dict(sorted(counts.items())), "snapshot_sha256": snapshot_sha,
            "source_bundle_sha256": bundle_sha, "canonical_state_unchanged": True,
            "fixture_contract": "explicit fragment instances and unmapped references; no explicit player",
            "native_readiness_accepted": False, "bytecode_executed": False,
            "retail_parity_accepted": False, "accepted_scenarios": []}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("probe", "frames", "loaded-scripts", "snapshot", "bindings-rust", "bindings-native", "bindings-bundle", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--negative-checks", action="store_true", help="Also require altered proof fields to fail this audit")
    args = parser.parse_args()
    raw = read(args.snapshot, 16 * 1024 * 1024)
    state, state_sha = snapshot(raw)
    inputs = [document(args.probe), document(args.frames), document(args.loaded_scripts), state,
              document(args.bindings_rust), document(args.bindings_native),
              sha(read(args.bindings_bundle, 512 * 1024 * 1024)), state_sha]
    result = compare(*inputs)
    if args.negative_checks:
        result["altered_evidence_rejections"] = negative_checks(inputs)
    # Input files are independently captured; retain their exact identities so
    # a publication wrapper can bind this audit to its frozen run.
    result["inputs"] = {name: sha(read(getattr(args, name.replace("-", "_")), 512 * 1024 * 1024))
                        for name in ("probe", "frames", "loaded-scripts", "snapshot", "bindings-rust", "bindings-native", "bindings-bundle")}
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Checked {result['selected_native_tuples_checked']} native tuples and saved storage outcomes")


def negative_checks(inputs):
    """Change captured evidence in place, restoring each field before the next.

    These are proof-boundary checks: a dropped occurrence, changed saved bits or
    a repaired finding must not turn into an accepted comparison.
    """
    probe, frames, loaded, _, _, native, _, _ = inputs
    prepared = next(row["probe"] for row in probe["events"] if row["probe"])
    unresolved = next(o["outcome"] for row in probe["events"] if row["probe"]
                      for o in row["probe"]["operands"] if o["outcome"]["status"] == "unresolved")
    local = next(o["outcome"]["resolution"] for row in probe["events"] if row["probe"]
                 for o in row["probe"]["operands"] if o["outcome"]["status"] == "resolved"
                 and o["outcome"]["resolution"]["kind"] == "local" and o["outcome"]["resolution"]["value"] is not None)
    finding = next(row for row in probe["events"] if row["finding"])
    changes = [
        ("saved_local_bits", local["value"], "bits", local["value"]["bits"] ^ 1),
        ("unresolved_status", unresolved, "code", "invented_success"),
        ("dropped_selected_use", prepared, "operands", prepared["operands"][:-1]),
        ("selected_window", prepared, "begin_scda_offset", prepared["begin_scda_offset"] + 1),
        ("full_binding_digest", prepared, "full_definition_binding_sha256", "0" * 64),
        ("loaded_source_cohort", loaded["plugins"][0], "source_sha256", "0" * 64),
        ("native_tuple_digest", native["compiled_units"][0], "binding_tuples_hex", "00" + native["compiled_units"][0]["binding_tuples_hex"][2:]),
        ("hidden_source_finding", finding, "finding", None),
    ]
    rejected = []
    for name, owner, field, changed in changes:
        original = owner[field]
        require(changed != original, f"Negative case makes no change: {name}")
        owner[field] = changed
        try:
            compare(*inputs)
        except RuntimeError:
            rejected.append(name)
        else:
            raise RuntimeError(f"Altered evidence was accepted: {name}")
        finally:
            owner[field] = original
    # Change the loader and probe together. An audit that uses only their
    # agreement would miss this; the independent native tuple must reject it.
    scripts = {key(s["handle"]): s for s in loaded["scripts"]}
    for row in probe["events"]:
        if row["probe"] is None:
            continue
        local_operand = next((o for o in row["probe"]["operands"] if o["outcome"]["status"] == "resolved"
                              and o["outcome"]["resolution"]["kind"] == "local"), None)
        if local_operand is not None:
            source = next(d for d in scripts[key(row["definition"])]["declarations"]
                          if d["index"] == local_operand["binding"]["index"])
            projected = local_operand["outcome"]["resolution"]["declaration"]
            original_source, original_projection = source["decoded_offset"], projected["declaration_decoded_offset"]
            source["decoded_offset"] ^= 1
            projected["declaration_decoded_offset"] = source["decoded_offset"]
            try:
                compare(*inputs)
            except RuntimeError as error:
                require(str(error) == "Loaded local declaration differs from native tuple", "Correlated projection failed for unrelated reason")
                rejected.append("correlated_loaded_and_probe_declaration")
            else:
                raise RuntimeError("Correlated altered storage projection was accepted")
            finally:
                source["decoded_offset"], projected["declaration_decoded_offset"] = original_source, original_projection
            break
    require(len(rejected) == 9, "Correlated declaration negative case was omitted")
    for row in probe["events"]:
        if row["probe"] is None:
            continue
        variable = next((o for o in row["probe"]["operands"] if o["binding"]["status"] == 3), None)
        if variable is not None:
            script = scripts[key(row["definition"])]
            source = next(d for d in script["declarations"] if d["index"] == variable["binding"]["target_value"])
            original = source["type_byte"]
            source["type_byte"] ^= 1
            try:
                compare(*inputs)
            except RuntimeError as error:
                require(str(error) == "Loaded local declaration differs from native tuple", "SCRV type negative failed for unrelated reason")
                rejected.append("loaded_scrv_type_with_unchanged_reference_kind")
            else:
                raise RuntimeError("Altered SCRV type projection was accepted")
            finally:
                source["type_byte"] = original
            break
    require(len(rejected) == 10, "SCRV type negative case was omitted")
    return rejected


if __name__ == "__main__":
    main()
