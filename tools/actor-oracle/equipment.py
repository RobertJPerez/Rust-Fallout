"""Independent equipment source roles from raw plugin/BSA bytes; no Rust input."""
import hashlib
import struct
from dependencies import fields, key_json, key_tuple, normalize, require, terminated, MIB


def role(text):
    armor = {"armor-male-biped": ("armor_biped", "male", "MODL"),
             "armor-female-biped": ("armor_biped", "female", "MOD3"),
             "armor-male-world": ("armor_world", "male", "MOD2"),
             "armor-female-world": ("armor_world", "female", "MOD4")}
    if text in armor:
        kind, sex, tag = armor[text]
        return dict(kind=kind, sex=sex), tag, {"ARMO", "ARMA"}
    tags = {"weapon-shell": "MOD2", "weapon-scope": "MOD3", "weapon-world": "MOD4"}
    if text in tags:
        return dict(kind=text.replace("-", "_")), tags[text], {"WEAP"}
    name, mask = text.split(":")
    mask = int(mask)
    require(name in {"weapon-model", "weapon-first-person"} and 0 <= mask <= 7, "explicit source role")
    tag = ("MODL" if not mask else f"MWD{mask}") if name == "weapon-model" else ("WNAM" if not mask else f"WNM{mask}")
    return dict(kind=name.replace("-", "_"), mod_mask=mask), tag, {"WEAP"}


def manifest(reader, actor, chosen, text, digest):
    selected_role, tag, allowed = role(text)
    context = dict(region=None, sex=None, part=None)
    counts = {name: 0 for name in ["decoded_bytes", "fields", "strings", "path_bytes", "bindings", "visits"]}
    lookup = {name: 0 for name in ["nodes", "model_source_nodes", "inventory_edges", "model_edges", "field_visits", "paths", "path_bytes", "candidates", "candidate_bytes", "cyclic_components", "cyclic_nodes"]}
    lookup["lookup_statuses"] = {}
    counts["lookup"] = lookup
    sources, links, requests, issues = [], [], [], []

    def origin(key, body):
        entry = reader.winners[key]
        return dict(plugin=reader.names[entry["source"]], sha256=reader.sources[entry["source"]].sha256,
            record_file_offset=entry["offset"], record_flags=entry["flags"],
            decoded_record_sha256=hashlib.sha256(body).hexdigest() if body is not None else None)

    def header(key):
        return {name: value for name, value in reader.winners[key].items() if name not in {"source", "kind_name"}}

    def source(key, read):
        entry = reader.winners[key]
        body = reader.payload(key, min(64*MIB, 128*MIB-counts["decoded_bytes"])) if read and not entry["flags"] & 0x20 else None
        result = dict(key=key_json(key), source=origin(key, body), header=header(key),
            deleted=bool(entry["flags"] & 0x20), fields=[], race_links=[], findings=[])
        if body is None:
            return result
        require(entry["version"] == 15, "unsupported equipment source version")
        counts["decoded_bytes"] += len(body)
        kind = entry["kind_name"]
        models = {"MODL"} if kind == "STAT" else {"MODL", "MOD2", "MOD3", "MOD4"}
        first = {"WNAM", *(f"WNM{i}" for i in range(1, 8))} if kind == "WEAP" else set()
        if kind == "WEAP":
            models.update(f"MWD{i}" for i in range(1, 8))
        stat_models = 0
        for name, offset, raw in fields(body):
            value = dict(kind="opaque")
            if name in models:
                path = terminated(raw)
                value = dict(kind="paths", role="model", context=context, strings=[dict(field_byte_offset=0, raw=list(path))])
                counts["strings"] += 1
                counts["path_bytes"] += len(path)
                if kind == "STAT":
                    stat_models += 1
                    if stat_models > 1:
                        result["findings"].append(dict(field_decoded_offset=offset, code="multiple_actor_model_fields"))
            if kind in {"ARMO", "ARMA"} and name == "BMDT":
                require(len(raw) == 8, "BMDT source shape")
                value = dict(kind="biped_slots", flags=struct.unpack_from("<I", raw)[0], general_flags=raw[4], unused=list(raw[5:]))
            if kind in {"ARMO", "ARMA", "WEAP"} and name == "ETYP":
                require(len(raw) == 4, "ETYP source shape")
                value = dict(kind="equipment_type", raw=struct.unpack("<i", raw)[0])
            if name in first:
                require(len(raw) == 4, "first-person source shape")
                binding = reader.binding(entry["source"], struct.unpack("<I", raw)[0])
                permitted = bytes(binding["target"]["kind"]) == b"STAT" if binding["target"] else None
                value = dict(kind="links", role="first_person_model", bindings=[dict(field_byte_offset=0, binding=binding, schema_kind_allowed=permitted)])
                counts["bindings"] += 1
            result["fields"].append(dict(kind=list(name.encode("latin1")), decoded_offset=offset,
                bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(), value=value))
            counts["fields"] += 1
            require(counts["fields"] <= 16384 and counts["strings"] <= 4096 and counts["path_bytes"] <= MIB and counts["bindings"] <= 4096, "equipment source limits")
        return result

    def issue(code, index=None, field=None):
        require(len(issues) < 128, "equipment issue limit")
        issues.append(dict(code=code, source_index=index, field_index=field))

    def model(index, wanted):
        definition = sources[index]
        counts["visits"] += len(definition["fields"])
        selected = [(i, f) for i, f in enumerate(definition["fields"]) if bytes(f["kind"]).decode() == wanted]
        if not selected:
            issue("missing_selected_model_field", index)
        if len(selected) > 1:
            issue("ambiguous_selected_model_fields", index)
        for i, field in selected:
            raw = bytes(field["value"]["strings"][0]["raw"])
            path, status, candidates = None, "empty_source_path", []
            if raw:
                if len(raw) > 4096:
                    status = "lookup_path_too_long"
                else:
                    try:
                        path = normalize(b"meshes/"+raw)
                        candidates = reader.mounts.get(path, [])
                        status = "archive_collision" if len(candidates)>1 else "one_archive_candidate" if candidates else "missing_archive_candidate"
                    except ValueError:
                        path, status = None, "unsafe_asset_path"
            lookup["paths"] += 1
            lookup["path_bytes"] += len(raw)
            lookup["candidates"] += len(candidates)
            lookup["candidate_bytes"] += sum(len(c["container"].encode())+len(c["original_path"]) for c in candidates)
            lookup["lookup_statuses"][status] = lookup["lookup_statuses"].get(status, 0)+1
            if status != "one_archive_candidate":
                issue("selected_model_asset_unavailable_or_ambiguous", index, i)
            requests.append(dict(source_index=index, role=selected_role, ambiguous_source=len(selected)>1,
                path=dict(source=definition["key"], field_index=i, field_decoded_offset=field["decoded_offset"],
                field_byte_offset=0, role="model", context=context, raw=list(raw), asset_path=list(path) if path is not None else None,
                lookup_status=status, candidates=candidates)))
            require(len(requests) <= 32, "equipment request limit")

    entry = reader.winners[actor]
    require(entry["kind_name"] in {"NPC_", "CREA"} and not entry["flags"] & 0x20, "equipment actor source")
    report = dict(sources=[dict(source_name=name, source_bytes=s.size, source_sha256=s.sha256) for name,s in zip(reader.names,reader.sources)],
        winning_content_sha256=digest, actor=dict(key=key_json(actor), source=origin(actor,reader.payload(actor,64*MIB)), header=header(actor)),
        explicit_choice=dict(equipment=key_json(chosen),role=selected_role), source_records=sources, selected_links=links,
        requests=requests, issues=issues, counts=counts, equipped_state_verified=False, effective_model_selection_supported=False,
        slot_conflicts_evaluated=False, texture_swaps_applied=False, attachment_target_selected=False, original_behavior_verified=False,
        scope="Explicit caller equipment/source role and winning model/slot/first-person declarations with physical archive candidates; no inferred equipped inventory, actor sex fallback, active weapon mods, slot conflicts, texture swaps, attachment node selection, NIF decoding, playback or retail model choice")
    if chosen not in reader.winners:
        issue("selected_equipment_missing")
        return report
    entry = reader.winners[chosen]
    sources.append(source(chosen,entry["kind_name"] in allowed))
    if entry["flags"] & 0x20:
        issue("selected_equipment_deleted",0)
        return report
    if entry["kind_name"] not in allowed:
        issue("selected_equipment_wrong_kind",0)
        return report
    counts["visits"] += len(sources[0]["fields"])
    for name in (["ETYP"] if entry["kind_name"] == "WEAP" else ["BMDT","ETYP"]):
        counts["visits"] += len(sources[0]["fields"])
        found = sum(bytes(f["kind"]).decode() == name for f in sources[0]["fields"])
        if not found:
            issue("missing_equipment_slot_field",0)
        if found>1:
            issue("ambiguous_equipment_slot_fields",0)
    if selected_role["kind"] != "weapon_first_person":
        model(0,tag)
        return report
    counts["visits"] += len(sources[0]["fields"])
    selected = [(i,f) for i,f in enumerate(sources[0]["fields"]) if bytes(f["kind"]).decode() == tag]
    if not selected:
        issue("missing_selected_first_person_link",0)
    if len(selected)>1:
        issue("ambiguous_selected_first_person_links",0)
    for i,field in selected:
        link=field["value"]["bindings"][0]
        target=None
        if len(selected)==1 and link["binding"]["status"]=="defined" and link["schema_kind_allowed"] is True:
            target=len(sources)
            sources.append(source(key_tuple(link["binding"]["key"]),True))
            model(target,"MODL")
        elif len(selected)==1:
            issue("selected_first_person_target_unavailable",0,i)
        links.append(dict(source_index=0,field_index=i,target_source_index=target,ambiguous_source=len(selected)>1))
        require(len(links)<=32,"equipment link limit")
    require(counts["visits"]<=65536,"equipment visit limit")
    return report
