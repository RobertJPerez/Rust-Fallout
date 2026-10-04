"""Compare complete stat requests with independent native and template readers.

Actor scalar values come from the frozen native raw-plugin reader. Template
origins come from dependencies.py's independent physical fields and graph. The
explicit canonical snapshot supplies only campaign/cohort/revision bookkeeping;
no observed Rust request contributes an expected field or source value.
"""
import argparse
import hashlib
import json
import pathlib


def load(path):
    raw = path.read_bytes()
    if len(raw) > 256 * 1024 * 1024:
        raise ValueError("stat request report byte budget")
    return json.loads(raw), dict(path=str(path), bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())


def key(value):
    return value["origin_plugin"].lower(), value["local_id"]


def expected(native, snapshot, root):
    actors = {key(row["key"]): row for row in native["definitions"]}
    actor = actors[root]
    if actor["deleted"]:
        raise ValueError("chosen actor deleted")
    templates = [row for row in native["actor_template_dependencies"]["manifests"] if key(row["root"]) == root]
    if len(templates) != 1:
        raise ValueError("one matching independently decoded template manifest required")
    template = templates[0]
    candidates = [dict(template_source_index=index, definition=actors[key(source["key"])])
                  for index, source in enumerate(template["candidate_sources"])]
    scalar_fields = sum(len(row["definition"]["fields"]) for row in candidates)
    # The independent scalar decoder retains every physical subrecord. Offsets
    # include extended prefixes; the final physical extent is the decoded body.
    decoded_bytes = sum(max((field["decoded_offset"] + 6 + field["bytes"]
                             for field in row["definition"]["fields"]), default=0)
                        for row in candidates)
    visits = template["field_visits"] + scalar_fields + 2 * len(actor["fields"])
    for source, candidate in zip(template["candidate_sources"], candidates):
        if source["configuration"] is not None:
            visits += len(candidate["definition"]["fields"])
    categories = {row["category"]: row for row in template["categories"]}
    root_source = next(row for row in template["candidate_sources"] if key(row["key"]) == root)
    configuration = root_source["configuration"]
    plain = dict(status="not_declared_for_component")
    auto = plain
    if actor["kind"] == list(b"NPC_"):
        auto = (dict(status="configuration_unavailable") if configuration is None else
                dict(status="npc_declaration", enabled=bool(configuration["flags"] & 0x10)))
    names = {
        "configuration": [("fatigue", "stats", plain), ("barter_gold", "ai_data", plain),
                          ("level_word", "stats", plain), ("player_level_multiplier_flag", "stats", plain),
                          ("calc_min", "stats", plain), ("calc_max", "stats", plain),
                          ("speed_multiplier", "stats", plain), ("karma_bits", "traits", plain),
                          ("disposition_base", "traits", plain)],
        "npc_data": [("base_health", "stats", plain), ("attributes", "stats", auto)],
        "npc_skills": [("skill_values", "stats", auto), ("skill_offsets", "stats", auto)],
        "creature_data": [("creature_type", "traits", plain), ("combat_skill", "stats", plain),
                          ("magic_skill", "stats", plain), ("stealth_skill", "stats", plain),
                          ("health", "stats", plain), ("damage", "stats", plain), ("attributes", "stats", plain)],
    }
    occurrences = {tag: sum(field["kind"] == list(tag.encode("ascii")) for field in actor["fields"])
                   for tag in ["ACBS", "DATA", "DNAM"]}
    requests = []
    for index, field in enumerate(actor["fields"]):
        if field["value"]["kind"] not in names:
            continue
        count = occurrences[bytes(field["kind"]).decode("ascii")]
        components = []
        for name, category, automatic in names[field["value"]["kind"]]:
            declaration = categories[category]
            available = (count == 1 and declaration["declaration"]["status"] == "authored_source"
                         and automatic["status"] != "configuration_unavailable"
                         and not automatic.get("enabled", False))
            components.append(dict(name=name, template_category=category, template_mask=declaration["mask"],
                                   template_flag_present=declaration["template_flag_present"],
                                   root_declaration_available=available, automatic_calculation=automatic,
                                   evaluated_value=None))
        requests.append(dict(scalar_field_index=index, field=field, ambiguous_source=count > 1, components=components))
    missing = [dict(kind=list(tag.encode("ascii")), code=code)
               for tag, code in [("ACBS", "configuration_unavailable"), ("DATA", "actor_data_unavailable"),
                                 ("DNAM", "npc_skills_unavailable")]
               if occurrences[tag] == 0 and (tag != "DNAM" or actor["kind"] == list(b"NPC_"))]
    return dict(campaign=snapshot["campaign"], source_cohort_sha256=snapshot["catalogue_sha256"],
                state_revision=snapshot["state_revision"], actor_key=actor["key"], actor_kind=actor["kind"],
                actor_source=actor["source"], template_request=template, candidate_scalars=candidates,
                fields=requests, missing_fields=missing, scalar_fields=scalar_fields, decoded_bytes=decoded_bytes,
                component_requests=sum(len(row["components"]) for row in requests), preparation_visits=visits,
                current_actor_values=None, actor_reference_bound=False, initialization_supported=False,
                automatic_calculation_supported=False, original_behavior_verified=False,
                scope="Exact authored ACBS/DATA/DNAM and template candidate origins; category declarations and NPC auto-calc source flag, no inherited/effective/current actor values, actor reference binding, initialization, level conversion or gameplay")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["native-report", "snapshot", "observed-report", "output"]:
        parser.add_argument("--" + name, type=pathlib.Path, required=True)
    parser.add_argument("--actor-root", required=True)
    args = parser.parse_args()
    native, native_receipt = load(args.native_report)
    snapshot, snapshot_receipt = load(args.snapshot)
    observed, observed_receipt = load(args.observed_report)
    root_plugin, root_id = args.actor_root.split(":")
    wanted = expected(native, snapshot, (root_plugin.lower(), int(root_id, 16)))
    observation = observed.get("stat_requests", observed)
    mismatched = sorted(set(wanted) | set(observation))
    mismatched = [name for name in mismatched if wanted.get(name) != observation.get(name)]
    result = dict(complete_observation_equal=not mismatched, mismatched_fields=mismatched,
                  native_input=native_receipt, snapshot_input=snapshot_receipt, observed_input=observed_receipt,
                  scalar_fields=wanted["scalar_fields"], component_requests=wanted["component_requests"],
                  gameplay_accepted=False)
    with args.output.open("x", encoding="utf-8") as target:
        json.dump(result, target, indent=2)
    if mismatched:
        raise SystemExit("stat request comparison differs: " + ", ".join(mismatched))


if __name__ == "__main__":
    main()
