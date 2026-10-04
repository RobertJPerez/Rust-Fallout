"""Independent physical effect requests over existing native actor producers.

Uses the locked source/header/FormID reader and exact native actor associations.
Expected SPEL/ENCH fields come from fresh bytes. MGEF bodies remain unread.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, fields, key_tuple, require, key_json
from context import physical

SCOPE = 'Explicit physical actor SPLO/EITM occurrence and ordered SPEL/ENCH EFID/EFIT declarations with MGEF winning source-header requests; exact unsigned/signed/raw flags and opaque conditions, no template inheritance, editor rewrite, active effects, stacking, timing, magnitude conversion or execution'
HEADER = ('kind', 'offset', 'stored_size', 'flags', 'form_id', 'revision', 'version', 'trailing_bytes')


def source_request(reader, key):
    entry = reader.winners[key]
    source = reader.sources[entry['source']]
    return dict(key=key_json(key), source_name=reader.names[entry['source']],
        source_bytes=source.size, source_sha256=source.sha256,
        header={name: entry[name] for name in HEADER})


def manifest(reader, native, inventory, root, field_index):
    candidates = [d for d in native['definitions'] if key_tuple(d['key']) == root]
    sources = [d for d in inventory['definitions'] if key_tuple(d['key']) == root]
    require(len(candidates) == len(sources) == 1, 'one native actor/inventory source')
    actor, source = candidates[0], sources[0]
    entry = reader.winners[root]
    require(not actor['deleted'] and entry['kind_name'] in ('NPC_', 'CREA'), 'live selected actor')
    require(source['source'] == actor['source'] and actor['source']['plugin'] == reader.names[entry['source']]
        and actor['source']['sha256'] == reader.sources[entry['source']].sha256
        and actor['source']['record_file_offset'] == entry['offset']
        and actor['source']['record_flags'] == entry['flags'], 'fresh actor header/source')
    raw = physical(reader, root, actor)
    require(len(raw) == len(source['fields']), 'native actor/inventory physical count')
    associated = next(d for d in native['actor_associations']['definitions'] if key_tuple(d['key']) == root)['associations']
    chosen = [a for a in associated if a['field_index'] == field_index]
    require(len(chosen) == 1 and chosen[0]['role'] in ('actor_effect', 'unarmed_effect'), 'explicit physical effect association')
    association = chosen[0]
    tag, offset, actor_raw = raw[field_index]
    require(tag == ('SPLO' if association['role'] == 'actor_effect' else 'EITM') and len(actor_raw) == 4, 'exact actor effect bytes')
    binding = reader.binding(entry['source'], struct.unpack('<I', actor_raw)[0])
    require(binding == association['binding'], 'fresh actor effect binding')
    allowed = None if binding['target'] is None else bytes(binding['target']['kind']) in ((b'SPEL',) if tag == 'SPLO' else (b'SPEL', b'ENCH'))
    require(allowed == association['schema_kind_allowed'], 'actor effect kind schema')
    configurations = [f for f in source['fields'] if f['kind'] == list(b'ACBS')]
    mask = configurations[0]['value']['template_flags'] if len(configurations) == 1 and configurations[0]['value']['kind'] == 'actor_base' else None
    template = None if mask is None else bool(mask & 8)
    repeated = tag == 'EITM' and sum(a['role'] == association['role'] for a in associated) > 1
    admitted = template is False and not repeated and binding['status'] == 'defined' and allowed is True
    issues = []
    if template is None:
        issues.append('unique_configuration_unavailable')
    if template:
        issues.append('actor_effect_template_inheritance_unsupported')
    if repeated:
        issues.append('repeated_unarmed_effect_association')
    if binding['status'] != 'defined':
        issues.append({'null': 'null_effect_association', 'missing': 'missing_effect_association_target', 'deleted': 'deleted_effect_association_target'}[binding['status']])
    if allowed is False:
        issues.append('effect_association_kind_not_allowed')
    visits = 2 * len(associated) + len(actor['fields']) + len(source['fields'])
    retained = len(actor['fields']) + len(configurations)
    selected, depth, bindings = 1, 0, 0
    decoded = len(reader.payload(root, 8 * MIB))
    raw_bytes = len(actor_raw)
    declaration = None
    if binding['status'] == 'defined' and allowed is True:
        target_key = key_tuple(binding['key'])
        request = source_request(reader, target_key)
        target = binding['target']
        require(target['kind'] == request['header']['kind'] and target['source_plugin'] == request['source_name']
            and target['record_file_offset'] == request['header']['offset']
            and target['record_flags'] == request['header']['flags'], 'exact declaration identity')
        body = reader.payload(target_key, MIB)
        require(len(body) + decoded <= 8 * MIB, 'decoded byte budget')
        decoded += len(body)
        selected, depth = 2, 1
        winner = reader.winners[target_key]
        supported = winner['version'] == 15
        metadata_tag = 'SPIT' if winner['kind_name'] == 'SPEL' else 'ENIT'
        declaration = dict(source=request, decoded_record_sha256=hashlib.sha256(body).hexdigest(),
            record_version_supported=supported, fields=[], metadata_field_indices=[], groups=[], issues=[])
        if not supported:
            declaration['issues'].append('unsupported_effect_record_version')
        for index, (kind, location, payload) in enumerate(fields(body)):
            reader.guard()
            visits += 1
            retained += 1
            raw_bytes += len(payload)
            require(visits <= 200000 and retained <= 65536 and raw_bytes <= MIB, 'field/byte budgets')
            value = dict(kind='opaque')
            if supported and kind == metadata_tag and len(payload) == 16:
                declaration['metadata_field_indices'].append(index)
                words = struct.unpack_from('<III', payload)
                value = dict(kind='spell_metadata', effect_type=words[0], cost_unused=words[1], level_unused=words[2],
                    flags=payload[12], unused=list(payload[13:16])) if kind == 'SPIT' else dict(kind='enchantment_metadata',
                    effect_type=words[0], unused_words=list(words[1:]), flags=payload[12], unused=list(payload[13:16]))
            elif supported and kind == 'EFIT' and len(payload) == 20:
                magnitude, area, duration, effect_type, actor_value = struct.unpack('<IIIIi', payload)
                value = dict(kind='effect_data', magnitude=magnitude, area=area, duration=duration,
                    effect_type=effect_type, actor_value=actor_value, known_effect_type=effect_type <= 2)
            if kind == metadata_tag and not (supported and len(payload) == 16):
                declaration['issues'].append('unsupported_effect_metadata_layout')
            if kind == 'EFID':
                require(len(declaration['groups']) < 4096, 'group budget')
                group = dict(efid_field_index=index, efit_field_indices=[], condition_field_indices=[], unknown_field_indices=[],
                    binding=None, schema_kind_allowed=None, base_effect=None, binding_admitted=False, data_admitted=False, issues=[])
                if supported and len(payload) == 4:
                    bindings += 1
                    require(bindings <= 4096, 'binding budget')
                    effect = reader.binding(winner['source'], struct.unpack('<I', payload)[0])
                    group['binding'] = effect
                    group['schema_kind_allowed'] = None if effect['target'] is None else effect['target']['kind'] == list(b'MGEF')
                    if effect['status'] != 'defined':
                        group['issues'].append({'null': 'null_base_effect', 'missing': 'missing_base_effect', 'deleted': 'deleted_base_effect'}[effect['status']])
                    if group['schema_kind_allowed'] is False:
                        group['issues'].append('base_effect_kind_not_allowed')
                    effect_key = None if effect['key'] is None else key_tuple(effect['key'])
                    if effect_key in reader.winners:
                        group['base_effect'] = source_request(reader, effect_key)
                        selected += 1
                        depth = 2
                        require(selected <= 4096, 'selected record budget')
                    group['binding_admitted'] = admitted and effect['status'] == 'defined' and group['schema_kind_allowed'] is True
                else:
                    group['issues'].append('unsupported_base_effect_link_layout_or_version')
                declaration['groups'].append(group)
            elif kind in ('EFIT', 'CTDA'):
                if not declaration['groups']:
                    declaration['issues'].append('effect_field_without_explicit_efid_anchor')
                else:
                    group = declaration['groups'][-1]
                    if kind == 'EFIT':
                        if group['condition_field_indices']:
                            group['issues'].append('effect_data_after_conditions')
                        group['efit_field_indices'].append(index)
                    else:
                        if len(group['efit_field_indices']) != 1:
                            group['issues'].append('condition_without_unique_effect_data')
                        group['condition_field_indices'].append(index)
            elif declaration['groups']:
                declaration['groups'][-1]['unknown_field_indices'].append(index)
            declaration['fields'].append(dict(kind=list(kind.encode('latin1')), decoded_offset=location,
                raw_bytes=list(payload), sha256=hashlib.sha256(payload).hexdigest(), value=value))
        if len(declaration['metadata_field_indices']) != 1:
            declaration['issues'].append('unique_effect_metadata_unavailable')
        if not declaration['groups']:
            declaration['issues'].append('missing_explicit_effect_groups')
        for group in declaration['groups']:
            visits += 1
            require(visits <= 200000, 'field visit budget')
            indices = group['efit_field_indices']
            if len(indices) != 1:
                group['issues'].append('unique_effect_data_unavailable')
            if group['unknown_field_indices']:
                group['issues'].append('unknown_effect_group_members')
            if group['condition_field_indices']:
                group['issues'].append('opaque_condition_requests_not_evaluated')
            data_known = len(indices) == 1 and declaration['fields'][indices[0]]['value'].get('known_effect_type') is True
            if len(indices) == 1 and not data_known:
                group['issues'].append('unsupported_effect_data_layout_or_type')
            group['data_admitted'] = supported and len(declaration['metadata_field_indices']) == 1 and not declaration['issues']
            group['data_admitted'] = bool(group['data_admitted'] and not group['unknown_field_indices'] and data_known
                and not any(issue in ('effect_data_after_conditions', 'condition_without_unique_effect_data') for issue in group['issues']))
    result = dict(sources=native['sources'], winning_content_sha256=native['winning_content_sha256'], actor=actor,
        configuration_fields=configurations, actor_effect_template_flag=template, association=association,
        actor_field=actor['fields'][field_index], actor_raw_bytes=list(actor_raw), singleton_repeated=repeated,
        declaration_binding_available=admitted, declaration=declaration, selected_records=selected, source_depth=depth,
        field_visits=visits, retained_fields=retained, decoded_bytes=decoded, raw_bytes=raw_bytes, bindings=bindings,
        issues=issues, active_effects_created=False, execution_supported=False, scope=SCOPE)
    require(len(native['sources']) <= 256 and len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode()) <= 16 * MIB, 'source/output budgets')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'inventory-report', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--root', required=True)
    parser.add_argument('--field-index', type=int, required=True)
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
            and control['generation'] == assignment['generation'] == lease['generation'] and lease['session_uuid'] == args.session_id, 'actor effect authorization changed')
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
        result = manifest(reader, native, inventory, (origin.lower(), int(local, 16)), args.field_index)
        if args.observed:
            require(args.observed.stat().st_size <= 256 * MIB, 'observed byte budget')
            observed = json.loads(args.observed.read_bytes())
            observed = observed['actor_effect_inputs']['manifest'] if 'actor_effect_inputs' in observed else observed
            require(result == observed, 'complete independent effect source projection differs')
        native['actor_effect_inputs'] = dict(manifest=result)
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(native, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True if args.observed else None, root=args.root, field_index=args.field_index,
            groups=0 if result['declaration'] is None else len(result['declaration']['groups']),
            bindings=result['bindings'], raw_bytes=result['raw_bytes'], retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
