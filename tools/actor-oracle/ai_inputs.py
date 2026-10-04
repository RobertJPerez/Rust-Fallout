"""Independent raw AIDT/ZNAM expectation using existing native source producers.

Reuses the locked Reader for source/header/FormID admission. No observed actor
request supplies expected fields; no combat-style body or behavior is decoded.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, fields, key_tuple, require

SCOPE = 'Exact authored AIDT words and explicit ZNAM/CSTY winning identity; raw enum/flag/signed/mood-unused source inputs only, no template inheritance, aggression/hostility/service/training/radius truth, combat-style formulas, live actor values, defaults or AI execution'


def manifest(reader, native, inventory, root):
    actors = [d for d in native['definitions'] if key_tuple(d['key']) == root]
    inventories = [d for d in inventory['definitions'] if key_tuple(d['key']) == root]
    require(len(actors) == len(inventories) == 1, 'one native actor/inventory source')
    actor, source = actors[0], inventories[0]
    entry = reader.winners[root]
    require(entry['kind_name'] in {'NPC_', 'CREA'} and not entry['flags'] & 0x20, 'live selected actor')
    body = reader.payload(root, 64 * MIB)
    require(actor['source'] == source['source'] and actor['source']['decoded_record_sha256'] == hashlib.sha256(body).hexdigest()
        and actor['source']['plugin'] == reader.names[entry['source']] and actor['source']['sha256'] == reader.sources[entry['source']].sha256
        and actor['source']['record_file_offset'] == entry['offset'] and actor['source']['record_flags'] == entry['flags'], 'fresh source body/header/hash')
    physical = list(fields(body))
    require(len(physical) == len(actor['fields']) == len(source['fields']), 'physical source field count')
    for (tag, offset, raw), declared in zip(physical, actor['fields']):
        require(declared['kind'] == list(tag.encode('latin1')) and declared['decoded_offset'] == offset
            and declared['bytes'] == len(raw) and declared['sha256'] == hashlib.sha256(raw).hexdigest(), 'physical field span/hash')
    supported = entry['version'] in ({14, 15} if entry['kind_name'] == 'NPC_' else {9, 11, 13, 14, 15})
    configurations = [f for f in source['fields'] if f['kind'] == list(b'ACBS')]
    template_flags = configurations[0]['value']['template_flags'] if len(configurations) == 1 and configurations[0]['value']['kind'] == 'actor_base' else None
    ai_template = None if template_flags is None else bool(template_flags & 16)
    traits_template = None if template_flags is None else bool(template_flags & 1)
    counts = {tag: sum(f[0] == tag for f in physical) for tag in ['AIDT', 'ZNAM']}
    require(3 * len(physical) <= 200000 and sum(counts.values()) <= 4096, 'selected work budget')
    issues = []
    if not supported:
        issues.append('unsupported_actor_record_version')
    if template_flags is None:
        issues.append('unique_configuration_unavailable')
    if ai_template:
        issues.append('ai_data_template_inheritance_unsupported')
    if traits_template:
        issues.append('combat_style_template_inheritance_unsupported')
    if not counts['AIDT']:
        issues.append('missing_ai_data')
    ai_data, styles, total = [], [], 0
    for index, (tag, offset, raw) in enumerate(physical):
        reader.guard()
        if tag not in counts:
            continue
        total += len(raw)
        require(total <= MIB, 'raw byte budget')
        findings = [] if supported else ['unsupported_actor_record_version']
        row = dict(field_index=index, field_decoded_offset=offset, raw_bytes=list(raw), repeated=counts[tag] > 1)
        if tag == 'AIDT':
            if row['repeated']:
                findings.append('repeated_ai_data')
            if len(raw) != 20:
                findings.append('unsupported_ai_data_layout')
            value = None
            if supported and len(raw) == 20:
                teaches = struct.unpack_from('<b', raw, 12)[0]
                assistance = struct.unpack_from('<b', raw, 14)[0]
                known = dict(aggression=raw[0] <= 3, confidence=raw[1] <= 4, mood=raw[4] <= 7,
                    teaches=-1 <= teaches <= 13, assistance=0 <= assistance <= 2, aggro_radius_behavior=raw[15] <= 1)
                value = dict(aggression=raw[0], confidence=raw[1], energy_level=raw[2], responsibility=raw[3], mood=raw[4],
                    mood_unused=list(raw[5:8]), services_flags=struct.unpack_from('<I', raw, 8)[0], teaches=teaches,
                    maximum_training_level=raw[13], assistance=assistance, aggro_radius_behavior=raw[15],
                    aggro_radius=struct.unpack_from('<i', raw, 16)[0], known_enums=known)
                for name, admitted in known.items():
                    if not admitted:
                        findings.append('unknown_' + name + '_enum')
            row.update(value=value, findings=findings)
            ai_data.append(row)
        else:
            if row['repeated']:
                findings.append('repeated_combat_style')
            if len(raw) != 4:
                findings.append('unsupported_combat_style_layout')
            binding = reader.binding(entry['source'], struct.unpack('<I', raw)[0]) if supported and len(raw) == 4 else None
            target = None if binding is None else binding['target']
            allowed = None if target is None else target['kind'] == list(b'CSTY')
            if binding and binding['status'] != 'defined':
                findings.append({'null': 'null_combat_style', 'missing': 'missing_combat_style', 'deleted': 'deleted_combat_style'}[binding['status']])
            if allowed is False:
                findings.append('combat_style_kind_not_allowed')
            winner = None if binding is None or binding['key'] is None else reader.winners.get(key_tuple(binding['key']))
            header = None if winner is None else {k: winner[k] for k in ('kind', 'offset', 'stored_size', 'flags', 'form_id', 'revision', 'version', 'trailing_bytes')}
            row.update(binding=binding, schema_kind_allowed=allowed, winning_header=header, binding_admitted=counts['ZNAM'] == 1
                and traits_template is False and allowed is True and binding is not None and binding['status'] == 'defined', findings=findings)
            styles.append(row)
    admitted = counts['AIDT'] == 1 and ai_template is False and ai_data[0]['value'] is not None and all(ai_data[0]['value']['known_enums'].values())
    result = dict(sources=native['sources'], winning_content_sha256=native['winning_content_sha256'], actor=actor,
        configuration_fields=configurations, ai_data_template_flag=ai_template, traits_template_flag=traits_template,
        record_version_supported=supported, ai_data=ai_data, combat_styles=styles, authored_ai_input_admitted=admitted,
        field_visits=3 * len(physical), raw_bytes=total, issues=issues, live_behavior_evaluated=False, execution_supported=False, scope=SCOPE)
    require(len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode('utf-8')) <= 16 * MIB, 'projection budget')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'inventory-report', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--root', required=True)
    parser.add_argument('--observed', type=pathlib.Path)
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
            and control['generation'] == assignment['generation'] == lease['generation'] and lease['session_uuid'] == args.session_id, 'actor AI authorization changed')
        for mailbox in team.glob('*.outbox.jsonl'):
            for line in mailbox.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != control['run_id'] or row.get('type') != 'stop_requested', 'STOP')

    guard()
    require(args.base_report.stat().st_size <= 256 * MIB and args.inventory_report.stat().st_size <= 256 * MIB, 'native input byte budget')
    native = json.loads(args.base_report.read_bytes())
    inventory = json.loads(args.inventory_report.read_bytes())
    require(native['sources'] == inventory['sources'] and native['winning_content_sha256'] == inventory['metadata']['winning_definitions_sha256'], 'native source cohorts differ')
    origin, local = args.root.rsplit(':', 1)
    reader = Reader(args.data, json.loads(args.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = manifest(reader, native, inventory, (origin.lower(), int(local, 16)))
        if args.observed:
            require(args.observed.stat().st_size <= 256 * MIB, 'observed byte budget')
            observed = json.loads(args.observed.read_bytes())
            observed = observed['actor_ai_inputs']['manifest'] if 'actor_ai_inputs' in observed else observed
            require(result == observed, 'complete independent AI source projection differs')
        native['actor_ai_inputs'] = dict(manifest=result)
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(native, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True if args.observed else None, root=args.root, ai_fields=len(result['ai_data']),
            combat_style_fields=len(result['combat_styles']), raw_bytes=result['raw_bytes'], retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
