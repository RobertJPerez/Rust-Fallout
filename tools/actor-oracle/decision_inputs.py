"""Independent selected actor groups and directed faction occurrence joins.

Reuse the protected whole-source Reader, native scalar/association/faction
declarations and the existing placement oracle. No runtime decoder or save
loader is introduced. Schema4 input supplies the literal current facts.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from context import expected as actor_context, physical
from dependencies import Reader, key_tuple, require, MIB

BATCH_SCOPE = 'Ordered explicit canonical actor references with one admitted whole source cohort and deterministic shared winning actor-base table; each authored placement and optional current state remain separate, no discovery, defaults, registration, retail initialization or mutation authority'
PAIR_SCOPE = 'Fresh explicit canonical actor endpoints and directed physical SNAM/FACT XNAM matching occurrences; repeated source ranks, signed modifiers and raw reaction words preserved, no effective membership, symmetry, same-faction defaults, reaction aggregation, disposition, reputation or gameplay truth'


def compact_bytes(value):
    return len(json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8'))


def batch_expected(reader, native, snapshot, selected):
    require(isinstance(selected, list) and len(selected) <= 4096 and len(set(selected)) == len(selected), 'bounded unique explicit selection')
    rows = [actor_context(reader, native, snapshot, ref) for ref in selected]
    bases = {key_tuple(row['actor']['key']): row['actor'] for row in rows}
    keys = sorted(bases)
    indices = {key: index for index, key in enumerate(keys)}
    references = [dict(reference=row['reference'], placement=row['placement'],
                       base_source_index=indices[key_tuple(row['actor']['key'])],
                       base_field_index=row['base_field_index'], transform_field_index=row['transform_field_index'],
                       fields=row['fields']) for row in rows]
    placement_fields = sum(len(row['placement']['fields']) for row in rows)
    actor_fields = sum(len(bases[key]['fields']) for key in keys)
    usage = dict(source_admissions=1, source_receipt_visits=2*len(native['sources']),
                 selected_references=len(selected), unique_bases=len(keys), placement_fields=placement_fields,
                 actor_fields=actor_fields, total_fields=placement_fields+actor_fields,
                 current_view_bytes=sum(compact_bytes(row['reference']) for row in rows),
                 visits=2*len(native['sources'])+len(selected)+placement_fields+actor_fields+7*len(selected))
    return dict(base_sources=[bases[key] for key in keys], references=references, usage=usage,
                actor_initialization_supported=False, authored_enable_evaluated=False, scope=BATCH_SCOPE)


def endpoint(reader, native, snapshot, reference):
    context = actor_context(reader, native, snapshot, reference)
    root = key_tuple(context['actor']['key'])
    actor = next(a for a in native['definitions'] if key_tuple(a['key']) == root)
    links = next(a for a in native['actor_associations']['definitions'] if key_tuple(a['key']) == root)
    faction_map = {key_tuple(f['key']): f for f in native['actor_factions']['definitions']}
    actor_raw = physical(reader, root, actor)
    memberships, issues = [], []
    configurations = [(index, data) for index, (tag, _, data) in enumerate(actor_raw) if tag == 'ACBS']
    if len(configurations) == 1:
        index, data = configurations[0]
        require(len(data) == 24, 'physical ACBS shape')
        if struct.unpack_from('<H', data, 22)[0] & 4:
            issues.append(dict(code='faction_template_selection_unsupported', inventory_field_index=index))
    elif not configurations:
        issues.append(dict(code='missing_actor_configuration', inventory_field_index=None))
    else:
        issues.append(dict(code='ambiguous_actor_configuration', inventory_field_index=None))
    issues.append(dict(code='current_membership_unsupported', inventory_field_index=None))
    faction_fields = source_relationships = 0
    for association in links['associations']:
        if association['role'] != 'faction':
            continue
        field_index = association['field_index']
        tag, _, data = actor_raw[field_index]
        require(tag == 'SNAM' and len(data) == 8, 'physical SNAM shape')
        raw, rank = struct.unpack_from('<Ib', data)
        source = reader.winners[root]['source']
        binding = reader.binding(source, raw)
        require(binding == association['binding'] and rank == association['faction_rank']
                and list(data[5:]) == association['faction_unused'], 'physical SNAM binding/rank/unused')
        faction = None
        if binding['status'] == 'defined' and association['schema_kind_allowed'] is True:
            faction = faction_map[key_tuple(binding['key'])]
            require(not faction['deleted'], 'winning FACT not deleted')
            faction_raw = physical(reader, key_tuple(faction['key']), faction)
            faction_fields += len(faction_raw)
            for index, (tag, _, data) in enumerate(faction_raw):
                if tag != 'XNAM':
                    continue
                require(len(data) == 12, 'physical XNAM shape')
                target, modifier, reaction = struct.unpack('<IiI', data)
                value = faction['fields'][index]['value']
                require(value['kind'] == 'relation' and value['modifier'] == modifier
                        and value['group_combat_reaction'] == reaction
                        and value['faction'] == reader.binding(reader.winners[key_tuple(faction['key'])]['source'], target),
                        'physical directed relation fields')
                source_relationships += 1
        memberships.append(dict(occurrence_index=len(memberships), association=association,
                                field=actor['fields'][field_index], faction=faction,
                                unavailable='current_membership_unsupported' if faction else 'authored_faction_unavailable'))
    visits = 2*len(actor_raw)+len(links['associations'])+faction_fields
    return dict(context=context, memberships=memberships, issues=issues, association_findings=links['findings']), faction_fields, source_relationships, visits, len(links['associations'])


def pair_expected(reader, native, snapshot, choice):
    require(set(choice) == {'from_reference', 'to_reference', 'expected_from_actor', 'expected_to_actor'}, 'strict explicit pair choice')
    from_, from_fields, from_relationships, from_visits, from_links = endpoint(reader, native, snapshot, choice['from_reference'])
    to, to_fields, _, to_visits, to_links = endpoint(reader, native, snapshot, choice['to_reference'])
    require(from_['context']['actor']['key'] == choice['expected_from_actor']
            and to['context']['actor']['key'] == choice['expected_to_actor'], 'fresh exact actor base claims')
    targets = {}
    for membership in to['memberships']:
        if membership['faction']:
            targets.setdefault(key_tuple(membership['faction']['key']), []).append(membership)
    relations, unavailable = [], []
    for membership in from_['memberships']:
        faction = membership['faction']
        if faction is None:
            continue
        for field_index, field in enumerate(faction['fields']):
            value = field['value']
            if value['kind'] != 'relation':
                continue
            if value['faction']['status'] != 'defined' or value['schema_kind_allowed'] is not True:
                unavailable.append(dict(source_occurrence_index=membership['occurrence_index'],
                                        relationship_field_index=field_index, field=field, code='authored_relation_target_unavailable'))
                continue
            for target in targets.get(key_tuple(value['faction']['key']), []):
                relations.append(dict(source_occurrence_index=membership['occurrence_index'],
                                      source_field_index=membership['association']['field_index'], source_faction=faction['key'],
                                      relationship_field_index=field_index, field=field,
                                      target_occurrence_index=target['occurrence_index'], target_field_index=target['association']['field_index'],
                                      target_faction=target['faction']['key'], modifier=value['modifier'], group_combat_reaction=value['group_combat_reaction']))
    context_visits = from_['context']['visits']+to['context']['visits']
    preflight = 2*len(native['sources'])+context_visits-4*len(native['sources'])+from_links+to_links
    visits = from_visits+to_visits+context_visits+preflight+len(to['memberships'])+from_fields+len(relations)
    visits += 3*(from_fields+len(relations)+len(unavailable))
    usage = dict(occurrences=len(from_['memberships'])+len(to['memberships']), faction_fields=from_fields+to_fields,
                 source_relationships=from_relationships, target_index_entries=sum(map(len, targets.values())),
                 relation_pairs=len(relations), unavailable_relations=len(unavailable),
                 current_view_bytes=compact_bytes(from_['context']['reference'])+compact_bytes(to['context']['reference']), visits=visits)
    return dict(from_=from_, to=to, relations=relations, unavailable_relations=unavailable, usage=usage,
                effective_membership_supported=False, reaction_evaluation_supported=False, scope=PAIR_SCOPE)


def expected(reader, native, snapshot, choice, mode):
    if mode == 'batch':
        return batch_expected(reader, native, snapshot, choice)
    result = pair_expected(reader, native, snapshot, choice)
    result['from'] = result.pop('from_')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'snapshot', 'choice', 'observed', 'output']:
        parser.add_argument('--'+name, type=pathlib.Path, required=True)
    parser.add_argument('--mode', choices=['batch', 'pair'], required=True)
    parser.add_argument('--team-directory', type=pathlib.Path)
    parser.add_argument('--session-id')
    args = parser.parse_args()
    def guard():
        if args.team_directory is None:
            return
        read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
        team = args.team_directory
        control, assignment, lease = read(team.parent/'team/control.json'), read(team/'actors.assignment.json'), read(team/'leases/actors.json')
        require(control['mode'] == 'active' and not control['stop_requested'] and assignment['state'] == 'active'
                and assignment['implementation_authorized'] and control['generation'] == assignment['generation'] == lease['generation']
                and control['run_id'] == assignment['run_id'] == lease['run_id'] and lease['session_uuid'] == args.session_id, 'authorization changed')
        for path in team.glob('*.outbox.jsonl'):
            for line in path.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != control['run_id'] or row.get('type') != 'stop_requested', 'STOP')
    guard()
    for path, maximum in [(args.base_report,256*MIB),(args.snapshot,32*MIB),(args.choice,MIB),(args.observed,256*MIB)]:
        require(path.stat().st_size <= maximum, 'input byte budget')
    read = lambda path: json.loads(path.read_bytes())
    native, snapshot, choice, observed = map(read, [args.base_report,args.snapshot,args.choice,args.observed])
    reader = Reader(args.data, read(args.load_order), native, guard)
    try:
        wanted = expected(reader,native,snapshot,choice,args.mode)
        label = 'actor_context_batch' if args.mode == 'batch' else 'actor_faction_pair'
        require(wanted == observed.get(label,observed), 'complete independently authored decision inputs differ')
        guard()
        with args.output.open('x',encoding='utf-8') as target:
            json.dump(wanted,target,ensure_ascii=False,separators=(',',':'))
        print(json.dumps(dict(equal=True,mode=args.mode,usage=wanted['usage'],retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
