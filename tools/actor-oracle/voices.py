"""Independent selected voice projection from native source rows and raw bytes.

Reuses the independent source/header/FormID reader, never production output.
The native actor/association/race definitions are checked against fresh physical
bytes before reuse. VTYP bodies and race voice pairs are read independently.
"""
import argparse
import hashlib
import json
import pathlib
import struct
import zlib
from dependencies import Reader, fields, key_json, key_tuple, plugin_name, require, MIB

SCOPE = "Exact authored actor VTCK/RNAM, sex-bound RACE voice declarations and winning VTYP physical flags; no template inheritance, effective voice/default-dialogue selection, audio language/path, dialogue conditions or gameplay"


def manifest(reader, native, root):
    actors = {key_tuple(row["key"]): row for row in native["definitions"]}
    associations = {key_tuple(row["key"]): row for row in native["actor_associations"]["definitions"]}
    races = {key_tuple(row["key"]): row for row in native["actor_races"]["definitions"]}
    actor = actors[root]
    require(not actor["deleted"], "voice actor deleted")

    def physical(definition):
        key = key_tuple(definition["key"])
        entry = reader.winners[key]
        source = definition["source"]
        require(source["plugin"] == reader.names[entry["source"]]
                and source["sha256"] == reader.sources[entry["source"]].sha256
                and source["record_file_offset"] == entry["offset"]
                and source["record_flags"] == entry["flags"], "voice native winner source differs")
        if "header" in definition:
            require(definition["header"] == {k: entry[k] for k in ("kind", "offset", "stored_size", "flags", "form_id", "revision", "version", "trailing_bytes")}, "voice native header differs")
        else:
            require(definition["kind"] == entry["kind"] and definition["record_version"] == entry["version"], "voice actor header differs")
        body = reader.payload(key, 64 * MIB)
        require(source["decoded_record_sha256"] == hashlib.sha256(body).hexdigest(), "voice native body hash differs")
        raw = list(fields(body))
        require(len(raw) == len(definition["fields"]), "voice native physical field count differs")
        for (tag, offset, data), field in zip(raw, definition["fields"]):
            require(field["kind"] == list(tag.encode("latin1")) and field["decoded_offset"] == offset
                    and field["bytes"] == len(data) and field["sha256"] == hashlib.sha256(data).hexdigest(), "voice native field differs")
        return body, raw

    actor_body, actor_fields = physical(actor)
    links = associations[root]["associations"]
    entry = reader.winners[root]
    for link in links:
        tag, _, data = actor_fields[link["field_index"]]
        require(link["binding"] == reader.binding(entry["source"], struct.unpack_from("<I", data)[0]), "voice native association differs")
        if link["role"] in {"voice", "race"}:
            require(tag == {"voice": "VTCK", "race": "RNAM"}[link["role"]], "voice native association role differs")
    counts = dict(decoded_bytes=len(actor_body), fields=len(actor_fields), declarations=0,
                  visits=2 * len(actor_fields) + len(links))
    result = dict(sources=native["sources"], winning_content_sha256=native["winning_content_sha256"], actor=actor,
                  configuration=None, authored_sex=None, traits_template_flag_present=None,
                  actor_voices=[], race_requests=[], voice_sources=[], issues=[], counts=counts,
                  template_inheritance_supported=False, effective_voice_selection_supported=False,
                  dialogue_truth_supported=False, audio_path_selection_supported=False,
                  original_behavior_verified=False, scope=SCOPE)

    def issue(code, key, index=None):
        require(len(result["issues"]) < 128, "voice independent issue budget")
        result["issues"].append(dict(code=code, source=key_json(key), field_index=index))

    def declaration():
        counts["declarations"] += 1
        require(counts["declarations"] <= 128, "voice independent declaration budget")

    def voice(binding, origin, index):
        target = binding["target"]
        if binding["status"] != "defined" or target["kind"] != list(b"VTYP"):
            issue("unavailable_voice_type", origin, index)
            return None
        key = key_tuple(binding["key"])
        for i, source in enumerate(result["voice_sources"]):
            if key_tuple(source["key"]) == key:
                return i
        require(len(result["voice_sources"]) < 3, "voice independent source budget")
        header = reader.winners[key]
        source = dict(key=key_json(key), source=dict(plugin=reader.names[header["source"]],
            sha256=reader.sources[header["source"]].sha256, record_file_offset=header["offset"],
            record_flags=header["flags"], decoded_record_sha256=None),
            header={k: header[k] for k in ("kind", "offset", "stored_size", "flags", "form_id", "revision", "version", "trailing_bytes")},
            deleted=False, fields=[], source_body_read=False)
        if header["version"] not in {1, 4, 9, 11, 13, 14, 15}:
            issue("unsupported_voice_type_version", key)
        else:
            body = reader.payload(key, min(64 * MIB, 128 * MIB - counts["decoded_bytes"]))
            flags = 0
            for tag, offset, data in fields(body):
                if tag == "DNAM":
                    require(len(data) == 1, "voice DNAM requires one byte")
                    flags += 1
                source["fields"].append(dict(kind=list(tag.encode("latin1")), decoded_offset=offset,
                    bytes=len(data), sha256=hashlib.sha256(data).hexdigest(), flags=data[0] if tag == "DNAM" else None))
            if flags > 1:
                issue("ambiguous_voice_type_flags", key)
            source["source"]["decoded_record_sha256"] = hashlib.sha256(body).hexdigest()
            source["source_body_read"] = True
            counts["decoded_bytes"] += len(body)
            counts["fields"] += len(source["fields"])
            counts["visits"] += len(source["fields"])
        result["voice_sources"].append(source)
        return len(result["voice_sources"]) - 1

    configurations = []
    for i, (tag, offset, data) in enumerate(actor_fields):
        if tag == "ACBS":
            require(len(data) == 24, "voice ACBS extent")
            configurations.append(dict(inventory_field_index=i, field_decoded_offset=offset,
                flags=struct.unpack_from("<I", data)[0], template_flags=struct.unpack_from("<H", data, 22)[0]))
    if len(configurations) == 1:
        result["configuration"] = configurations[0]
        inherited = bool(configurations[0]["template_flags"] & 1)
        result["traits_template_flag_present"] = inherited
        if inherited:
            issue("traits_template_selection_unsupported", root)
        elif actor["kind"] == list(b"NPC_"):
            result["authored_sex"] = "female" if configurations[0]["flags"] & 1 else "male"
    else:
        issue("missing_actor_configuration" if not configurations else "ambiguous_actor_configuration", root)
    voice_count = sum(link["role"] == "voice" for link in links)
    race_count = sum(link["role"] == "race" for link in links)
    if not voice_count:
        issue("missing_actor_voice_declaration", root)
    if actor["kind"] == list(b"NPC_") and not race_count:
        issue("missing_actor_race_declaration", root)
    for link in links:
        if link["role"] not in {"voice", "race"}:
            continue
        declaration()
        i = link["field_index"]
        field = actor["fields"][i]
        if link["role"] == "voice":
            if voice_count > 1:
                issue("ambiguous_actor_voice_declaration", root, i)
            result["actor_voices"].append(dict(association=link, field=field, ambiguous_source=voice_count > 1,
                target_source_index=None if voice_count > 1 else voice(link["binding"], root, i)))
            continue
        request = dict(association=link, field=field, ambiguous_source=race_count > 1, definition=None, voices=[])
        if race_count > 1:
            issue("ambiguous_actor_race_declaration", root, i)
        elif link["binding"]["status"] != "defined" or link["schema_kind_allowed"] is not True:
            issue("unavailable_actor_race", root, i)
        else:
            key = key_tuple(link["binding"]["key"])
            definition = races[key]
            body, raw = physical(definition)
            request["definition"] = definition
            counts["decoded_bytes"] += len(body)
            counts["fields"] += len(raw)
            counts["visits"] += len(raw)
            count = sum(tag == "VTCK" for tag, _, _ in raw)
            if not count:
                issue("missing_race_voice_declaration", key)
            for i, (tag, _, data) in enumerate(raw):
                if tag != "VTCK":
                    continue
                require(len(data) == 8, "race voice pair extent")
                if count > 1:
                    issue("ambiguous_race_voice_declaration", key, i)
                for sex, word in zip(("male", "female"), struct.unpack("<II", data)):
                    declaration()
                    binding = reader.binding(reader.winners[key]["source"], word)
                    request["voices"].append(dict(race_field_index=i, field=definition["fields"][i], sex=sex,
                        binding=binding, ambiguous_source=count > 1,
                        matches_authored_actor_sex=None if result["authored_sex"] is None else result["authored_sex"] == sex,
                        target_source_index=None if count > 1 else voice(binding, key, i)))
        result["race_requests"].append(request)
    require(counts["decoded_bytes"] <= 128 * MIB and counts["fields"] <= 16384 and counts["visits"] <= 65536,
            "voice independent aggregate budget")
    require(len(json.dumps(result, separators=(",", ":")).encode()) <= 32 * MIB, "voice independent projection budget")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", type=pathlib.Path, required=True)
    parser.add_argument("--load-order", type=pathlib.Path, required=True)
    parser.add_argument("--base-report", type=pathlib.Path, required=True)
    parser.add_argument("--voice-root", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--team-directory", type=pathlib.Path)
    parser.add_argument("--session-id")
    args = parser.parse_args()

    def guard():
        if args.team_directory is None:
            return
        read = lambda path: json.loads(path.read_text(encoding="utf-8-sig"))
        team = args.team_directory
        control, assignment, lease = read(team.parent / "team/control.json"), read(team / "actors.assignment.json"), read(team / "leases/actors.json")
        require(control["mode"] == "active" and not control["stop_requested"] and assignment["state"] == "active"
                and assignment["implementation_authorized"] and control["run_id"] == assignment["run_id"] == lease["run_id"]
                and control["generation"] == assignment["generation"] and lease["session_uuid"] == args.session_id, "voice oracle authorization changed")
        for mailbox in team.glob("*.outbox.jsonl"):
            for line in mailbox.read_text(encoding="utf-8-sig").splitlines():
                if line.strip():
                    row = json.loads(line)
                    require(row.get("run_id") != control["run_id"] or row.get("type") != "stop_requested", "voice oracle observed STOP")
    guard()
    names = json.loads(args.load_order.read_text(encoding="utf-8-sig"))
    require(isinstance(names, list) and 0 < len(names) <= 256, "voice load order")
    with args.base_report.open("rb") as source:
        raw = source.read(256 * MIB + 1)
    require(len(raw) <= 256 * MIB, "voice native report budget")
    native = json.loads(raw)
    origin, local = args.voice_root.split(":")
    root = (plugin_name(origin), int(local, 16))
    require(0 < root[1] <= 0xFFFFFF, "voice root")
    reader = Reader(args.data, names, native, guard)
    try:
        native["actor_voice_requests"] = dict(manifest=manifest(reader, native, root))
        guard()
        with args.output.open("x", encoding="utf-8") as output:
            json.dump(native, output, separators=(",", ":"), ensure_ascii=True)
        print(json.dumps(dict(actor=key_json(root), counts=native["actor_voice_requests"]["manifest"]["counts"],
            input_receipts=[dict(path=str(s.path), bytes=s.size, sha256=s.sha256) for s in reader.sources],
            original_behavior_verified=False)))
    finally:
        reader.close()


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, zlib.error, struct.error) as error:
        raise SystemExit(f"actor-voice-oracle: {error}") from error
