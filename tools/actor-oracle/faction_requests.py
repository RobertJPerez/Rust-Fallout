"""Compare faction request source inputs against the independent native reader.

The native report must be produced from original plugin bytes using
--include-associations --include-factions. This join calls no production decoder.
Runtime campaign/reference existence and exact work budgets are Rust host tests;
this comparison establishes only physical authored source provenance and values.
"""
import argparse
import hashlib
import json
import pathlib


def load(path):
    raw = path.read_bytes()
    if len(raw) > 256 * 1024 * 1024:
        raise ValueError("faction request report byte budget")
    return json.loads(raw), {"path": str(path), "bytes": len(raw),
                             "sha256": hashlib.sha256(raw).hexdigest()}


def key(value):
    return value["origin_plugin"].lower(), value["local_id"]


def expected(native, root):
    actors = {key(row["key"]): row for row in native["definitions"]}
    links = {key(row["key"]): row for row in native["actor_associations"]["definitions"]}
    factions = {key(row["key"]): row for row in native["actor_factions"]["definitions"]}
    actor, associations = actors[root], links[root]
    if actor["deleted"]:
        raise ValueError("chosen actor deleted")
    occurrences, field_count, relation_count = [], 0, 0
    for association in associations["associations"]:
        if association["role"] != "faction":
            continue
        source = None
        binding = association["binding"]
        if binding["status"] == "defined" and association["schema_kind_allowed"] is True:
            definition = factions[key(binding["key"])]
            if definition["deleted"]:
                raise ValueError("defined FACT deleted")
            relations = [dict(faction_field_index=index, field=field,
                              evaluated_modifier=None, evaluated_reaction=None)
                         for index, field in enumerate(definition["fields"])
                         if field["value"]["kind"] == "relation"]
            source = dict(definition=definition, relations=relations)
            field_count += len(definition["fields"])
            relation_count += len(relations)
        occurrences.append(dict(association=association,
                                field=actor["fields"][association["field_index"]],
                                faction=source, live_membership=None, effective_rank=None))
    configurations = [[index, field] for index, field in enumerate(actor["fields"])
                      if field["kind"] == list(b"ACBS")]
    return dict(actor_key=actor["key"], actor_source=actor["source"],
                authored_configuration=configurations[0] if len(configurations) == 1 else None,
                association_findings=associations["findings"], factions=occurrences,
                faction_fields=field_count, relationship_requests=relation_count,
                template_selection_supported=False, membership_initialization_supported=False,
                relationship_evaluation_supported=False, condition_truth_supported=False,
                original_behavior_verified=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-report", type=pathlib.Path, required=True)
    parser.add_argument("--observed-report", type=pathlib.Path, required=True)
    parser.add_argument("--actor-root", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    native, native_receipt = load(args.native_report)
    observed, observed_receipt = load(args.observed_report)
    observation = observed.get("faction_requests", observed)
    origin, local = args.actor_root.split(":")
    root = origin.lower(), int(local, 16)
    wanted = expected(native, root)
    mismatches = [name for name, value in wanted.items() if observation.get(name) != value]
    result = dict(source_projection_equal=not mismatches, mismatched_fields=mismatches,
                  physical_occurrences=len(wanted["factions"]),
                  relationship_requests=wanted["relationship_requests"],
                  native_input=native_receipt, observed_input=observed_receipt,
                  runtime_state_verified=False, retail_parity_accepted=False)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    if mismatches:
        raise SystemExit("faction request source comparison differs: " + ", ".join(mismatches))


if __name__ == "__main__":
    main()
