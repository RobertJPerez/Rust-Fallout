"""Independent actor/RACE/CLAS join over existing native producers and fresh bytes.

The supplied snapshot contributes explicit canonical context only. This oracle
does not initialize actor values or restore an authoritative mutable World.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, key_tuple, require
from context import physical

SCOPE = 'Exact physical NPC RNAM/CNAM links and existing RACE/CLAS scalar declarations; both sex arrays/raw signed/float bits, no effective selection, template inheritance, current actor values, default or initialization formula'
OBSERVATION_SCOPE = 'Campaign/source-sealed exact authored initialization inputs; no bound actor reference, current/effective values, defaults, race/class application or mutation'


def origin(reader, key, definition, kind):
    entry = reader.winners[key]
    source = definition['source']
    require(entry['kind_name'] == kind and source['plugin'] == reader.names[entry['source']]
        and source['sha256'] == reader.sources[entry['source']].sha256
        and source['record_file_offset'] == entry['offset'] and source['record_flags'] == entry['flags'], 'exact winning native source')
    if 'header' in definition:
        require(definition['header'] == {k: entry[k] for k in ('kind', 'offset', 'stored_size', 'flags', 'form_id', 'revision', 'version', 'trailing_bytes')}, 'exact target header')
    return entry


def manifest(reader, native, inventory, root):
    actor = next(d for d in native['definitions'] if key_tuple(d['key']) == root)
    source = next(d for d in inventory['definitions'] if key_tuple(d['key']) == root)
    require(not actor['deleted'] and actor['kind'] in (list(b'NPC_'), list(b'CREA')), 'one live actor')
    entry = origin(reader, root, actor, bytes(actor['kind']).decode('ascii'))
    require(source['source'] == actor['source'], 'native inventory/actor source differs')
    raw = physical(reader, root, actor)
    require(len(raw) == len(source['fields']), 'inventory field count')
    configurations = [f for f in source['fields'] if f['kind'] == list(b'ACBS')]
    mask = configurations[0]['value']['template_flags'] if len(configurations) == 1 and configurations[0]['value']['kind'] == 'actor_base' else None
    traits = None if mask is None else bool(mask & 1)
    associations = next(d for d in native['actor_associations']['definitions'] if key_tuple(d['key']) == root)['associations']
    counts = {role: sum(d['role'] == role for d in associations) for role in ('race', 'class')}
    require(len(native['sources']) <= 256 and sum(counts.values()) <= 4096, 'source/link budget')
    issues = []
    npc = actor['kind'] == list(b'NPC_')
    if not npc:
        issues.append('race_class_roles_not_applicable_to_creature')
    if mask is None:
        issues.append('unique_configuration_unavailable')
    if traits:
        issues.append('traits_template_inheritance_unsupported')
    if npc:
        if not counts['race']:
            issues.append('missing_race_link')
        if not counts['class']:
            issues.append('missing_class_link')
    visits = 3 * len(raw) + 2 * len(associations)
    selected = len(raw) + len(configurations)
    decoded = len(reader.payload(root, 8 * MIB))
    links = []
    for association in associations:
        role = association['role']
        if role not in counts:
            continue
        require(npc, 'NPC-only race/class roles')
        index = association['field_index']
        tag, offset, value = raw[index]
        require(tag == ('RNAM' if role == 'race' else 'CNAM') and len(value) == 4, 'physical association')
        require(association['binding'] == reader.binding(entry['source'], struct.unpack('<I', value)[0]), 'independent canonical binding')
        link_issues = []
        repeated = counts[role] > 1
        if repeated:
            link_issues.append('repeated_initialization_link')
        binding = association['binding']
        if binding['status'] != 'defined':
            link_issues.append({'null': 'null_initialization_link', 'missing': 'missing_initialization_target', 'deleted': 'deleted_initialization_target'}[binding['status']])
        if association['schema_kind_allowed'] is False:
            link_issues.append('initialization_target_kind_not_allowed')
        target = None
        available = False
        if binding['target'] is not None and binding['target']['kind'] == list(b'RACE' if role == 'race' else b'CLAS'):
            key = key_tuple(binding['key'])
            target = next(d for d in native['actor_races' if role == 'race' else 'actor_classes']['definitions'] if key_tuple(d['key']) == key)
            origin(reader, key, target, 'RACE' if role == 'race' else 'CLAS')
            if not target['deleted']:
                physical(reader, key, target)
                decoded += len(reader.payload(key, 8 * MIB))
            visits += len(target['fields'])
            selected += len(target['fields'])
            available = not target['deleted'] and not target['findings']
            if not available:
                link_issues.append('target_scalar_inputs_unavailable')
        selected += 1
        direct = not repeated and traits is False and binding['status'] == 'defined' and association['schema_kind_allowed'] is True
        links.append(dict(association=association, field=actor['fields'][index], raw_bytes=list(value), repeated=repeated,
            race_definition=target if role == 'race' else None, class_definition=target if role == 'class' else None,
            direct_binding_available=direct, initialization_inputs_available=direct and available, issues=link_issues))
    require(visits <= 200000 and selected <= 65536 and decoded <= 8 * MIB, 'selected work budget')
    result = dict(sources=native['sources'], winning_content_sha256=native['winning_content_sha256'], actor=actor,
        configuration_fields=configurations, traits_template_flag=traits, links=links, selected_fields=selected,
        field_visits=visits, decoded_bytes=decoded, issues=issues, initialization_supported=False, scope=SCOPE)
    require(len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode('utf-8')) <= 16 * MIB, 'projection budget')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'inventory-report', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--root', required=True)
    parser.add_argument('--observed', type=pathlib.Path)
    parser.add_argument('--snapshot', type=pathlib.Path)
    parser.add_argument('--team-directory', type=pathlib.Path)
    parser.add_argument('--session-id')
    args = parser.parse_args()
    def guard():
        if args.team_directory is None:
            return
        team = args.team_directory
        read = lambda p: json.loads(p.read_text(encoding='utf-8-sig'))
        control, assignment, lease = read(team.parent / 'team/control.json'), read(team / 'actors.assignment.json'), read(team / 'leases/actors.json')
        require(control['mode'] == 'active' and not control['stop_requested'] and assignment['state'] == 'active'
            and assignment['implementation_authorized'] and control['run_id'] == assignment['run_id'] == lease['run_id']
            and control['generation'] == assignment['generation'] == lease['generation'] and lease['session_uuid'] == args.session_id, 'initialization source authorization changed')
        for mailbox in team.glob('*.outbox.jsonl'):
            for line in mailbox.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != control['run_id'] or row.get('type') != 'stop_requested', 'STOP')
    guard()
    require(args.base_report.stat().st_size <= 256 * MIB and args.inventory_report.stat().st_size <= 256 * MIB, 'native input budget')
    native = json.loads(args.base_report.read_bytes())
    inventory = json.loads(args.inventory_report.read_bytes())
    require(native['sources'] == inventory['sources'] and native['winning_content_sha256'] == inventory['metadata']['winning_definitions_sha256'], 'native cohorts differ')
    origin_name, local = args.root.rsplit(':', 1)
    reader = Reader(args.data, json.loads(args.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = manifest(reader, native, inventory, (origin_name.lower(), int(local, 16)))
        if args.snapshot:
            require(args.snapshot.stat().st_size <= 32 * MIB, 'snapshot budget')
            snapshot = json.loads(args.snapshot.read_bytes())
            require(snapshot['schema_version'] == 4 and snapshot['profile'] == native['profile']
                and len(snapshot['catalogue_sha256']) == 64, 'explicit canonical fixture metadata')
            result = dict(manifest=result, campaign=snapshot['campaign'], source_cohort_sha256=snapshot['catalogue_sha256'],
                state_revision=snapshot['state_revision'], current_actor_values=None, actor_reference_bound=False,
                initialization_supported=False, scope=OBSERVATION_SCOPE)
        if args.observed:
            require(args.observed.stat().st_size <= 256 * MIB, 'observed budget')
            observed = json.loads(args.observed.read_bytes())
            if args.snapshot:
                observed = observed.get('initialization_inputs', observed)
            elif 'actor_initialization_inputs' in observed:
                observed = observed['actor_initialization_inputs']['manifest']
            require(result == observed, 'complete independent initialization inputs differ')
        if args.snapshot:
            output = result
        else:
            native['actor_initialization_inputs'] = dict(manifest=result)
            output = native
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(output, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True if args.observed else None, root=args.root, retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
