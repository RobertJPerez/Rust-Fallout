"""Independent physical PACK condition/script reader over locked source bytes.

Augments a native actor oracle projection. No production decoder or executable
handler is called. Only descriptor bytes from the exact pinned PE are read.
"""
import argparse
import hashlib
import json
import pathlib
import struct
import zlib

from dependencies import MIB, Reader, Source, fields, key_json, key_tuple, require

EXECUTABLE_SHA256 = "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
SCRIPT_FIELDS = {"SCHR", "SCDA", "SCTX", "SLSD", "SCVR", "SCRO", "SCRV"}
FORM_DOMAINS = {3: "inventory_object", 4: "reference", 6: "actor", 9: "cell", 11: "effect_item", 14: "quest", 15: "race", 16: "class", 17: "faction", 19: "global", 20: "furniture", 21: "base_object", 25: "actor_base", 27: "worldspace", 29: "package", 31: "base_effect", 33: "weather", 35: "owner", 37: "form_list", 39: "perk", 40: "note", 47: "encounter_zone", 48: "idle", 50: "inventory_object", 53: "base_object", 61: "form", 62: "reputation", 63: "casino", 65: "challenge", 69: "region"}
SIGNED_DOMAINS = {5: "actor_value", 23: "quest_stage"}
UNSIGNED_DOMAINS = {8: "axis", 18: "sex", 28: "crime_type", 32: "form_type", 41: "misc_stat", 51: "alignment", 52: "equip_type", 55: "critical_stage"}


def compact(value):
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def word(raw, at=0):
    return struct.unpack_from("<I", raw, at)[0]


def display_kind(kind):
    return "".join(chr(b) if 33 <= b <= 126 else "\\x%02X" % b for b in kind.encode("latin1"))

def signed(raw):
    return raw if raw < 0x80000000 else raw - 0x100000000


def executable_receipt(path):
    source = Source(path)
    try:
        require(source.size <= 64 * MIB, "package descriptor executable byte budget")
        require(source.sha256 == EXECUTABLE_SHA256, "unsupported package descriptor executable fingerprint")
        raw = source.read(0, source.size)
        require(raw[:2] == b"MZ", "descriptor DOS header")
        pe = word(raw, 0x3c)
        require(raw[pe:pe + 4] == b"PE\0\0", "descriptor PE signature")
        machine, count = struct.unpack_from("<HH", raw, pe + 4)
        optional_size = struct.unpack_from("<H", raw, pe + 20)[0]
        optional = pe + 24
        require(machine == 0x14c and struct.unpack_from("<H", raw, optional)[0] == 0x10b and count <= 96, "descriptor PE32 profile")
        image_base = word(raw, optional + 28)
        sections = []
        for i in range(count):
            at = optional + optional_size + i * 40
            virtual_size, rva, size, offset = struct.unpack_from("<IIII", raw, at + 8)
            require(offset <= len(raw) and size <= len(raw) - offset, "descriptor PE section extent")
            sections.append((rva, virtual_size, offset, size))

        def offset_for(address, length):
            require(address >= image_base, "descriptor address below image")
            rva = address - image_base
            matches = [offset + rva - start for start, _, offset, size in sections if start <= rva and rva - start <= size and length <= size - (rva - start)]
            require(len(matches) == 1, "descriptor address mapping")
            return matches[0]

        def name(address):
            at = offset_for(address, 1)
            end = raw.find(b"\0", at, at + 129)
            require(at <= end < at + 128 and all(32 <= c <= 126 for c in raw[at:end]), "descriptor parameter name")
            offset_for(address, end - at + 1)
            return raw[at:end].decode("ascii")

        start = offset_for(0x01190910, 640 * 40)
        descriptors = []
        for function in range(640):
            at = start + function * 40
            if word(raw, at + 32) == 0:
                continue
            parameters = []
            count = struct.unpack_from("<H", raw, at + 18)[0]
            require(count <= 64, "descriptor parameter count")
            if count:
                parameter_start = offset_for(word(raw, at + 20), count * 12)
                for index in range(count):
                    p = parameter_start + index * 12
                    parameters.append({"descriptor_file_offset": p, "type_name": name(word(raw, p)), "type_id": word(raw, p + 4), "optional_word": word(raw, p + 8)})
            descriptors.append({"function_id": function, "descriptor_file_offset": at, "parameters": parameters})
        return {"source_bytes": source.size, "source_sha256": source.sha256, "source_version_profile": "authorized local NV 1.4.0.525; exact executable digest", "image_base": image_base, "pe_timestamp": word(raw, pe + 8), "condition_descriptors": descriptors}
    finally:
        source.close()


def dependency(reader, source, raw):
    key = reader.resolve(source, raw)
    entry = reader.winners.get(key)
    runtime = "nv-player-reference" if key == ("falloutnv.esm", 0x14) and entry is None else None
    target = None if entry is None else {"source_name": reader.names[entry["source"]], "record_kind": display_kind(entry["kind_name"]), "record_file_offset": entry["offset"], "record_flags": entry["flags"]}
    status = "null" if key is None else "deleted" if entry and entry["flags"] & 0x20 else "defined" if entry else "runtime_dependency" if runtime else "missing"
    return {"raw_word": raw, "key": key_json(key), "status": status, "runtime_binding": runtime, "target": target}


def typed_word(function, index, raw, parameter, first):
    def value(kind, domain=None):
        row = {"kind": kind, "raw_word": raw}
        if kind in {"signed_domain", "signed_integer"}:
            row["value"] = signed(raw)
        if kind == "variable_index":
            row["signed_index"] = signed(raw)
        if domain is not None:
            row["domain"] = domain
        return row
    if function == 408:
        if parameter != 1:
            return value("unknown")
        if index == 0:
            return value("unsigned_domain", "vats_function")
        choices = {0: ("form_id", "weapon"), 1: ("form_id", "form_list"), 3: ("form_id", "form_list"), 10: ("form_id", "form_list"), 2: ("form_id", "actor_base"), 5: ("signed_domain", "actor_value"), 6: ("unsigned_domain", "vats_action"), 9: ("form_id", "effect_item"), 15: ("unsigned_domain", "weapon_type")}
        return value(*choices[first]) if first in choices else value("unused" if first in {4, 7, 8, 11, 12, 13, 14, 16, 17} else "unknown")
    specials = {(420, 1, 1): ("signed_domain", "quest_objective"), (421, 1, 1): ("signed_domain", "quest_objective"), (36, 0, 1): ("unsigned_domain", "menu_mode"), (438, 0, 1): ("unsigned_domain", "creature_type"), (398, 0, 1): ("signed_domain", "body_location")}
    if (function, index, parameter) in specials:
        return value(*specials[function, index, parameter])
    if function == 427:
        return value("form_id", "voice_type") if parameter == 46 else value("unknown")
    if parameter in FORM_DOMAINS:
        return value("form_id", FORM_DOMAINS[parameter])
    if parameter in SIGNED_DOMAINS:
        return value("signed_domain", SIGNED_DOMAINS[parameter])
    if parameter in UNSIGNED_DOMAINS:
        return value("unsigned_domain", UNSIGNED_DOMAINS[parameter])
    return value({1: "signed_integer", 2: "float_bits", 22: "variable_index"}.get(parameter, "unknown"))


def condition(reader, source, raw, at, previous, signatures):
    require(len(raw) in {20, 24, 28}, "PACK CTDA width outside verified 20/24/28 layouts")
    flags, function = raw[0], struct.unpack_from("<H", raw, 8)[0]
    words = [word(raw, 12), word(raw, 16)]
    run_on = word(raw, 20) if len(raw) >= 24 else None
    reference = word(raw, 24) if len(raw) >= 28 else None
    signature = signatures.get(function)
    status = "missing_descriptor" if signature is None else "unverified_optional_word" if any(p["optional_word"] > 1 for p in signature) else "additional_parameters" if len(signature) > 2 else "descriptor_bound"
    operands = []
    for index, raw_word in enumerate(words):
        p = signature[index] if signature is not None and index < len(signature) else None
        value = typed_word(function, index, raw_word, p["type_id"], words[0]) if status == "descriptor_bound" and p else {"kind": "unused" if status == "descriptor_bound" else "unknown", "raw_word": raw_word}
        operands.append({"parameter_type_id": None if p is None else p["type_id"], "optional_word": None if p is None else p["optional_word"], "value": value, "form_dependency": dependency(reader, source, raw_word) if value["kind"] == "form_id" else None})
    if status == "descriptor_bound" and any(p["value"]["kind"] == "unknown" for p in operands):
        status = "schema_disagreement"
    if function in {106, 285}:
        subject = {"kind": "animation_group", "raw_word": run_on}
    elif run_on == 2:
        subject = {"kind": "reference", "raw_word": reference}
    elif run_on in {None, 0, 1, 3, 4}:
        subject = {"kind": {None: "absent", 0: "subject", 1: "target", 3: "combat_target", 4: "linked_reference"}[run_on]}
    else:
        subject = {"kind": "unknown", "raw_word": run_on}
    binding = {"signature_status": status, "signature_parameter_count": None if signature is None else len(signature), "operands": operands, "comparison_global": dependency(reader, source, word(raw, 4)) if flags & 4 else None, "subject": subject, "subject_reference": dependency(reader, source, reference) if function not in {106, 285} and run_on == 2 and reference is not None else None, "legacy_target_flag_present": bool(flags & 2), "live_values_resolved": False, "evaluation_ready": False}
    findings = []
    for present, code in [(flags & 0x1a, "uninterpreted_low_flags"), (raw[1:4] != b"\0" * 3, "nonzero_flag_padding"), (raw[10:12] != b"\0" * 2, "nonzero_function_padding"), (flags >> 5 > 5, "unknown_comparison_operator"), (not flags & 4 and word(raw, 4) & 0x7f800000 == 0x7f800000, "nonfinite_comparison"), (subject["kind"] == "unknown", "unknown_subject_selector")]:
        if present:
            findings.append(code)
    site = {"field_decoded_offset": at, "preceding_field_kind": None if previous is None else display_kind(previous[0]), "preceding_field_decoded_offset": None if previous is None else previous[1], "raw_bytes": list(raw), "sha256": digest(raw), "binding": binding, "source_findings": findings}
    operator = ["equal", "not_equal", "greater", "greater_or_equal", "less", "less_or_equal"][flags >> 5] if flags >> 5 <= 5 else {"unknown": flags >> 5}
    legacy = {"field_decoded_offset": at, "bytes": len(raw), "sha256": digest(raw), "preceding_field_kind": site["preceding_field_kind"], "preceding_field_decoded_offset": site["preceding_field_decoded_offset"], "flags": flags, "flag_padding": list(raw[1:4]), "function_id": function, "function_padding": list(raw[10:12]), "comparison_operator": operator, "comparison_value": {"global_raw_form" if flags & 4 else "float_bits": word(raw, 4)}, "or_flag": bool(flags & 1), "parameter_words": words, "run_on_word": run_on, "reference_word": reference, "binding": binding, "source_findings": findings}
    return site, len(raw) + len(compact(legacy))


def script_unit(reader, key, entry, body_hash, unit):
    metadata = hashlib.sha256()
    declarations, references, first = [], [], {}
    pending, compiled = None, None
    compiled_present = source_present = False
    for tag, at, raw in unit["fields"]:
        metadata.update(tag.encode("ascii") + struct.pack("<II", at, len(raw)) + raw)
        if pending is not None and tag != "SCVR":
            raise ValueError("SLSD has no following SCVR")
        if tag == "SCDA":
            require(not compiled_present, "duplicate SCDA in one script unit")
            compiled_present, compiled = True, raw
            # Independently bound the physical instruction envelopes; no opcode
            # or expression behavior is inferred from a successfully framed body.
            cursor = 0
            while cursor < len(raw):
                require(cursor + 4 <= len(raw), "compiled instruction header")
                opcode = struct.unpack_from("<H", raw, cursor)[0]
                if opcode == 0x1c:
                    cursor += 4
                    require(cursor + 4 <= len(raw), "compiled calling-reference header")
                length = struct.unpack_from("<H", raw, cursor + 2)[0]
                cursor += 4
                require(length <= len(raw) - cursor, "compiled instruction extent")
                if opcode == 0x10:
                    require(length >= 6, "compiled BEGIN header")
                cursor += length
        elif tag == "SCTX":
            require(not source_present, "duplicate SCTX in one script unit")
            source_present = True
        elif tag == "SLSD":
            require(len(raw) == 24 and len(declarations) < 65536, "SLSD layout/declaration budget")
            pending = at, raw
        elif tag == "SCVR":
            require(pending is not None and raw.endswith(b"\0") and b"\0" not in raw[:-1], "SCVR must follow SLSD and contain one terminated name")
            offset, declaration = pending
            index = word(declaration)
            first.setdefault(index, offset)
            declarations.append({"index": index, "type_byte": declaration[16], "decoded_offset": offset, "name_bytes": len(raw), "name_sha256": digest(raw)})
            pending = None
        elif tag in {"SCRO", "SCRV"}:
            require(len(raw) == 4 and len(references) < 65536, "reference entry layout/budget")
            references.append((tag, at, word(raw)))
    require(pending is None, "SLSD has no following SCVR")
    rows = []
    for index, (tag, at, raw) in enumerate(references, 1):
        row = {"index": index, "decoded_offset": at, "source_kind": tag, "value": raw, "status": "null_form", "form_key": None, "target": None, "variable_declaration_offset": None, "runtime_dependency": None}
        if tag == "SCRV":
            row["variable_declaration_offset"] = first.get(raw)
            row["status"] = "dynamic_variable" if raw in first else "missing_variable_declaration"
        else:
            resolved = dependency(reader, entry["source"], raw)
            row["form_key"] = resolved["key"]
            row["status"] = {"defined": "defined_form", "deleted": "deleted_form", "missing": "missing_form", "null": "null_form", "runtime_dependency": "runtime_dependency"}[resolved["status"]]
            row["runtime_dependency"] = resolved["runtime_binding"]
            target = resolved["target"]
            if target:
                row["target"] = {"source_plugin": target["source_name"], "record_file_offset": target["record_file_offset"], "record_flags": target["record_flags"], "record_kind": target["record_kind"]}
        rows.append(row)
    header = unit["fields"][0][2]
    issues = []
    if word(header, 4) != len(rows):
        issues.append("SCHR reference count differs from ordered table length")
    if word(header, 8) != (0 if compiled is None else len(compiled)):
        issues.append("SCHR compiled size differs from SCDA extent")
    version = {"source_plugin": reader.names[entry["source"]], "source_sha256": reader.sources[entry["source"]].sha256, "record_file_offset": entry["offset"], "record_flags": entry["flags"], "decoded_record_sha256": body_hash, "metadata_sha256": metadata.hexdigest(), "compiled_sha256": None if compiled is None else digest(compiled), "compiled_bytes": None if compiled is None else len(compiled)}
    identity = {"record": key_json(key), "header_decoded_offset": unit["offset"]}
    version_hash = hashlib.sha256(b"FRSCRV01")
    def text(value):
        encoded = value.encode("utf-8")
        version_hash.update(struct.pack("<I", len(encoded)) + encoded)
    text("nv-original")
    text(key[0])
    version_hash.update(struct.pack("<II", key[1], unit["offset"]))
    text(version["source_plugin"])
    text(version["source_sha256"])
    version_hash.update(struct.pack("<QI", entry["offset"], entry["flags"]))
    text(body_hash)
    text(version["metadata_sha256"])
    version_hash.update(bytes([compiled is not None]))
    if compiled is not None:
        text(version["compiled_sha256"])
        version_hash.update(struct.pack("<Q", len(compiled)))
    return {"field_index": unit["index"], "physical_marker": unit["marker"], "handle": {"key": identity, "version_sha256": version_hash.hexdigest()}, "version": version, "owner": {"kind": "unverified_embedded", "section_marker": unit["offset"], "stage_marker": None, "schema_ownership_verified": False}, "script_type": struct.unpack_from("<H", header, 16)[0], "flags": struct.unpack_from("<H", header, 18)[0], "declarations": declarations, "references": rows, "issues": issues}


def project(reader, native, descriptors, selected_keys=None):
    signatures = {row["function_id"]: row["parameters"] for row in descriptors["condition_descriptors"]}
    sources = [{"source_name": s["source_name"], "source_bytes": s["source_bytes"], "source_sha256": s["source_sha256"]} for s in native["sources"]]
    cohort = digest(b"FNVCTDASOURCES1" + compact(sources))
    expected = {key_tuple(d["key"]): d for d in native["actor_packages"]["definitions"]
                if selected_keys is None or key_tuple(d["key"]) in selected_keys}
    selected = []
    for key, entry in sorted(reader.winners.items()):
        if entry["kind_name"] == "PACK" and (selected_keys is None or key in selected_keys):
            require(len(selected) < 65536, "package candidate budget")
            selected.append((key, entry))
    require(set(expected) == {key for key, _ in selected}, "PACK winner identities differ from native base")
    counts = dict.fromkeys(["records", "deleted_records", "decoded_bytes", "fields", "field_visits", "conditions", "condition_retained_bytes", "event_fields", "event_links", "scripts", "compiled_scripts", "declarations", "references", "source_findings", "projection_bytes"], 0)
    result = []
    for key, entry in selected:
        reader.guard()
        parent = expected[key]
        header = {name: value for name, value in entry.items() if name not in {"source", "kind_name"}}
        require(header == parent["header"], "PACK native/raw winner header differs")
        source = {"plugin": reader.names[entry["source"]], "sha256": reader.sources[entry["source"]].sha256, "record_file_offset": entry["offset"], "record_flags": entry["flags"], "decoded_record_sha256": None}
        deleted = bool(entry["flags"] & 0x20)
        definition = {"key": key_json(key), "source": source, "header": header, "deleted": deleted, "conditions": None, "event_fields": [], "scripts": [], "findings": []}
        if deleted:
            counts["deleted_records"] += 1
        else:
            body = reader.sources[entry["source"]].body(entry, min(64 * MIB, 256 * MIB - counts["decoded_bytes"]))
            body_hash = digest(body)
            source["decoded_record_sha256"] = body_hash
            require(source == parent["source"], "PACK native/raw source/body digest differs")
            counts["decoded_bytes"] += len(body)
            prepared = {"identity": {"key": key_json(key), "source_name": source["plugin"], "source_sha256": source["sha256"], "source_cohort_sha256": cohort, "record_kind": "PACK", "record_file_offset": entry["offset"], "record_flags": entry["flags"], "decoded_bytes": len(body), "decoded_sha256": body_hash}, "fields": 0, "sites": [], "retained_bytes": 0}
            marker, previous, units = None, None, []
            for index, (tag, offset, raw) in enumerate(fields(body)):
                require(counts["fields"] < 2_000_000 and counts["field_visits"] + 2 <= 4_000_000, "package physical field/visit budget")
                counts["fields"] += 1
                counts["field_visits"] += 2
                prepared["fields"] += 1
                if tag == "CTDA":
                    require(counts["conditions"] < 1_000_000, "package condition row budget")
                    site, retained = condition(reader, entry["source"], raw, offset, previous, signatures)
                    require(retained <= 128 * MIB - counts["condition_retained_bytes"], "package condition retained budget")
                    prepared["sites"].append(site)
                    prepared["retained_bytes"] += retained
                    counts["conditions"] += 1
                    counts["condition_retained_bytes"] += retained
                    counts["source_findings"] += len(site["source_findings"])
                if tag in {"POBA", "POEA", "POCA", "INAM", "TNAM"}:
                    require(counts["event_fields"] < 1_000_000, "package event field budget")
                    if tag in {"POBA", "POEA", "POCA"}:
                        require(not raw, "package event marker must contain zero bytes")
                        marker = {"kind": list(tag.encode("ascii")), "field_index": index, "field_decoded_offset": offset}
                        value = {"kind": "marker"}
                    else:
                        require(len(raw) == 4 and counts["event_links"] < 1_000_000, "package event link must contain four bytes / link budget")
                        binding = reader.binding(entry["source"], word(raw))
                        target = binding["target"]
                        allowed = None if target is None else target["kind"] == list(("IDLE" if tag == "INAM" else "DIAL").encode("ascii"))
                        value = {"kind": "link", "binding": binding, "schema_kind_allowed": allowed}
                        if marker is None:
                            definition["findings"].append({"field_decoded_offset": offset, "code": "package_event_link_without_marker"})
                        if allowed is False:
                            definition["findings"].append({"field_decoded_offset": offset, "code": "package_event_link_schema_kind_mismatch"})
                        counts["event_links"] += 1
                    definition["event_fields"].append({"field_index": index, "field_kind": list(tag.encode("ascii")), "field_decoded_offset": offset, "bytes": len(raw), "sha256": digest(raw), "physical_marker": marker, "value": value})
                    counts["event_fields"] += 1
                if tag in SCRIPT_FIELDS:
                    if tag == "SCHR":
                        require(len(raw) == 20 and counts["scripts"] + len(units) < 262144, "SCHR layout/package script budget")
                        units.append({"index": index, "offset": offset, "marker": marker, "fields": []})
                    require(units, "script field has no SCHR owner")
                    units[-1]["fields"].append((tag, offset, raw))
                previous = tag, offset
            require(prepared["fields"] == len(parent["fields"]), "PACK native/raw field counts differ")
            definition["conditions"] = prepared
            for unit in units:
                script = script_unit(reader, key, entry, body_hash, unit)
                require(len(script["declarations"]) <= 262144 - counts["declarations"] and len(script["references"]) <= 1_000_000 - counts["references"], "package aggregate declaration/reference budget")
                counts["scripts"] += 1
                counts["compiled_scripts"] += script["version"]["compiled_sha256"] is not None
                counts["declarations"] += len(script["declarations"])
                counts["references"] += len(script["references"])
                counts["source_findings"] += len(script["issues"])
                definition["scripts"].append(script)
        counts["source_findings"] += len(definition["findings"])
        size = len(compact(definition))
        require(size <= 128 * MIB - counts["projection_bytes"], "package projection byte budget")
        counts["projection_bytes"] += size
        counts["records"] += 1
        result.append(definition)
    return {"counts": counts, "definitions": result, "descriptor_receipt": descriptors}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["data", "load-order", "base-report", "executable", "output"]:
        parser.add_argument("--" + name, required=True, type=pathlib.Path)
    parser.add_argument("--team-directory", type=pathlib.Path)
    parser.add_argument("--session-id")
    args = parser.parse_args()
    def guard():
        if args.team_directory is None:
            return
        read = lambda p: json.loads(p.read_text(encoding="utf-8-sig"))
        team = args.team_directory
        control, assignment, lease = read(team.parent / "team/control.json"), read(team / "actors.assignment.json"), read(team / "leases/actors.json")
        require(control["mode"] == "active" and not control["stop_requested"] and assignment["state"] == "active" and assignment["implementation_authorized"] and control["run_id"] == assignment["run_id"] == lease["run_id"] and lease["session_uuid"] == args.session_id, "package oracle authorization changed")
        for mailbox in team.glob("*.outbox.jsonl"):
            for line in mailbox.read_text(encoding="utf-8-sig").splitlines():
                if line.strip():
                    row = json.loads(line)
                    require(row.get("run_id") != control["run_id"] or row.get("type") != "stop_requested", "package oracle observed STOP")
    guard()
    names = json.loads(args.load_order.read_text(encoding="utf-8-sig"))
    require(isinstance(names, list) and 0 < len(names) <= 256, "package load order")
    with args.base_report.open("rb") as source:
        raw = source.read(256 * MIB + 1)
    require(len(raw) <= 256 * MIB, "package native base report byte budget")
    native = json.loads(raw)
    del raw
    descriptors = executable_receipt(args.executable)
    reader = Reader(args.data, names, native, guard)
    try:
        native["actor_package_dependencies"] = project(reader, native, descriptors)
        guard()
        encoder = json.JSONEncoder(separators=(",", ":"), ensure_ascii=False)
        retained = 0
        with args.output.open("xb") as output:
            for chunk in encoder.iterencode(native):
                value = chunk.encode("utf-8")
                require(len(value) <= 256 * MIB - retained, "package oracle report byte budget")
                output.write(value)
                retained += len(value)
                if retained % MIB < len(value):
                    guard()
        print(json.dumps({"counts": native["actor_package_dependencies"]["counts"], "input_receipts": [{"path": str(s.path), "bytes": s.size, "sha256": s.sha256} for s in reader.sources], "descriptor_receipt": {k: v for k, v in descriptors.items() if k != "condition_descriptors"}, "retail_parity_accepted": False}))
    finally:
        reader.close()


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, zlib.error, struct.error) as error:
        raise SystemExit(f"actor-package-dependencies-oracle: {error}") from error
