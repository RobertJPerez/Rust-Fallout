"""Independent raw WEAP/explicit-AMMO/PROJ source graph over the locked reader.

No observed request seeds expected fields. Existing source admission and FormID
resolution are reused; leaf, list, script, model and effect bodies stay unread.
"""
import argparse
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, fields, key_tuple, key_json, require

SCOPE = 'Explicit WEAP and optional caller AMMO physical declarations, independent weapon/ammo projectile inputs and winning header-only leaf links; raw signed/unsigned/float bits/flags, no inferred ammo or list membership, projectile precedence/fallback, editor normalization, fire/reload/consumption/damage/spread/ballistics/mod effects or execution'
HEADER = ('kind', 'offset', 'stored_size', 'flags', 'form_id', 'revision', 'version', 'trailing_bytes')
ALLOWED = dict(weapon_ammo={'AMMO', 'FLST'}, weapon_projectile={'PROJ'}, ammo_projectile={'PROJ'},
    critical_effect={'SPEL'}, vats_effect={'SPEL'}, consumed_ammo={'AMMO', 'MISC'}, ammo_effect={'AMEF'},
    light={'LIGH'}, muzzle_light={'LIGH'}, explosion={'EXPL'}, sound={'SOUN'}, countdown_sound={'SOUN'}, disable_sound={'SOUN'}, default_weapon={'WEAP'})


def manifest(reader, native, weapon, ammo):
    require(weapon in reader.winners and reader.winners[weapon]['kind_name'] == 'WEAP' and not reader.winners[weapon]['flags'] & 0x20, 'explicit live weapon')
    counts = dict(nodes=0, source_depth=0, headers=0, decoded_bytes=0, field_visits=0, fields=0, bindings=0, words=0, raw_bytes=0)

    def source(key):
        if key not in reader.winners:
            return None
        entry = reader.winners[key]
        origin = reader.sources[entry['source']]
        counts['headers'] += 1
        require(counts['headers'] <= 4096, 'header budget')
        return dict(key=key_json(key), source_name=reader.names[entry['source']], source_bytes=origin.size,
            source_sha256=origin.sha256, header={name: entry[name] for name in HEADER})

    def words(raw, start):
        result = [dict(field_byte_offset=at, raw=struct.unpack_from('<I', raw, at)[0]) for at in range(start, len(raw) - 3, 4)]
        counts['words'] += len(result)
        require(counts['words'] <= 65536, 'word budget')
        return result

    def decode(key, depth):
        reader.guard()
        require(depth <= 2 and counts['nodes'] < 64, 'body depth/node budget')
        counts['source_depth'] = max(depth, counts['source_depth'])
        counts['nodes'] += 1
        provenance = source(key)
        entry = reader.winners[key]
        body = reader.payload(key, min(MIB, 8 * MIB - counts['decoded_bytes']))
        counts['decoded_bytes'] += len(body)
        supported = entry['version'] == 15
        node = dict(source=provenance, decoded_record_sha256=hashlib.sha256(body).hexdigest(), record_version_supported=supported,
            fields=[], links=[], issues=[] if supported else [dict(field_index=None, field_kind=None, code='unsupported_attack_record_version')])
        for index, (tag, offset, raw) in enumerate(fields(body)):
            reader.guard()
            counts['field_visits'] += 1
            counts['fields'] += 1
            counts['raw_bytes'] += len(raw)
            require(counts['field_visits'] <= 200000 and counts['fields'] <= 65536 and counts['raw_bytes'] <= MIB, 'field/raw budget')
            value, links = dict(kind='opaque'), []
            pair = entry['kind_name'], tag
            layouts = {('WEAP', 'NAM0'): (4,), ('WEAP', 'DATA'): (15,), ('WEAP', 'DNAM'): (120, 204),
                ('WEAP', 'CRDT'): (16,), ('WEAP', 'VATS'): (16, 20), ('AMMO', 'DATA'): (13,),
                ('AMMO', 'DAT2'): (12, 16, 20), ('AMMO', 'RCIL'): (4,), ('PROJ', 'DATA'): (68, 80, 84)}
            if supported and pair in layouts:
                if len(raw) not in layouts[pair]:
                    node['issues'].append(dict(field_index=index, field_kind=list(tag.encode('ascii')), code='unsupported_attack_field_layout'))
                elif pair == ('WEAP', 'NAM0'):
                    links = [(0, 'weapon_ammo')]
                elif pair == ('WEAP', 'DATA'):
                    price, health, weight, damage, clip = struct.unpack('<iiIhB', raw)
                    value = dict(kind='weapon_data', value=price, health=health, weight_bits=weight, base_damage=damage, clip_size=clip)
                elif pair == ('WEAP', 'DNAM'):
                    links = [(36, 'weapon_projectile')]
                    value = dict(kind='weapon_attack', raw_words=words(raw, 0), flags1=raw[12], grip_animation=raw[13], ammo_use=raw[14],
                        reload_animation=raw[15], vats_to_hit_chance=raw[40], attack_animation=raw[41], projectile_count=raw[42],
                        embedded_actor_value=raw[43], skill=struct.unpack_from('<i', raw, 104)[0],
                        resist_type=struct.unpack_from('<i', raw, 120)[0] if len(raw) == 204 else None)
                elif pair == ('WEAP', 'CRDT'):
                    links = [(12, 'critical_effect')]
                    value = dict(kind='weapon_critical', critical_damage=struct.unpack_from('<H', raw)[0], unused_prefix=list(raw[2:4]),
                        multiplier_bits=struct.unpack_from('<I', raw, 4)[0], flags=raw[8], unused_suffix=list(raw[9:12]))
                elif pair == ('WEAP', 'VATS'):
                    links = [(0, 'vats_effect')]
                    value = dict(kind='weapon_vats', skill_bits=struct.unpack_from('<I', raw, 4)[0], damage_multiplier_bits=struct.unpack_from('<I', raw, 8)[0],
                        action_points_bits=struct.unpack_from('<I', raw, 12)[0], silent=raw[16] if len(raw) == 20 else None,
                        mod_required=raw[17] if len(raw) == 20 else None, unused=list(raw[18:20]) if len(raw) == 20 else None)
                elif pair == ('AMMO', 'DATA'):
                    value = dict(kind='ammo_data', speed_bits=struct.unpack_from('<I', raw)[0], flags=raw[4], unused=list(raw[5:8]),
                        value=struct.unpack_from('<i', raw, 8)[0], clip_rounds=raw[12])
                elif pair == ('AMMO', 'DAT2'):
                    links = [(4, 'ammo_projectile')] + ([(12, 'consumed_ammo')] if len(raw) >= 16 else [])
                    value = dict(kind='ammo_attack', projectiles_per_shot=struct.unpack_from('<I', raw)[0], weight_bits=struct.unpack_from('<I', raw, 8)[0],
                        consumed_percentage_bits=struct.unpack_from('<I', raw, 16)[0] if len(raw) == 20 else None)
                elif pair == ('AMMO', 'RCIL'):
                    links = [(0, 'ammo_effect')]
                else:
                    flags, projectile_type = struct.unpack_from('<HH', raw)
                    value = dict(kind='projectile_data', flags=flags, projectile_type=projectile_type,
                        known_projectile_type=projectile_type in (1, 2, 4, 8, 16), raw_words=words(raw, 4))
                    links = [(16, 'light'), (20, 'muzzle_light'), (36, 'explosion'), (40, 'sound'),
                        (56, 'countdown_sound'), (60, 'disable_sound'), (64, 'default_weapon')]
            for byte_offset, role in links:
                counts['bindings'] += 1
                require(counts['bindings'] <= 4096, 'binding budget')
                binding = reader.binding(entry['source'], struct.unpack_from('<I', raw, byte_offset)[0])
                allowed = None if binding['target'] is None else bytes(binding['target']['kind']).decode('ascii') in ALLOWED[role]
                issues = [] if binding['status'] == 'defined' else [dict(null='null_attack_source_link', missing='missing_attack_source_link', deleted='deleted_attack_source_link')[binding['status']]]
                if allowed is False:
                    issues.append('attack_source_kind_not_allowed')
                if role == 'weapon_ammo' and binding['target'] is not None and binding['target']['kind'] == list(b'FLST'):
                    issues.append('ammo_list_membership_unavailable')
                node['links'].append(dict(field_index=index, field_byte_offset=byte_offset, role=role, binding=binding, schema_kind_allowed=allowed,
                    target=None if binding['key'] is None else source(key_tuple(binding['key'])), target_node=None, repeated=False,
                    binding_admitted=binding['status'] == 'defined' and allowed is True, issues=issues))
            node['fields'].append(dict(kind=list(tag.encode('latin1')), decoded_offset=offset, raw_bytes=list(raw),
                sha256=hashlib.sha256(raw).hexdigest(), value=value))
        counts['field_visits'] += 2 * len(node['fields']) + len(node['links'])
        occurrences = {tag: sum(field['kind'] == list(tag.encode()) for field in node['fields']) for tag in ('NAM0', 'DATA', 'DNAM', 'CRDT', 'VATS', 'DAT2')}
        for index, field in enumerate(node['fields']):
            if occurrences.get(bytes(field['kind']).decode('latin1'), 0) > 1:
                node['issues'].append(dict(field_index=index, field_kind=field['kind'], code='repeated_singleton_source_field'))
        for link in node['links']:
            link['repeated'] = link['role'] != 'ammo_effect' and occurrences.get(bytes(node['fields'][link['field_index']]['kind']).decode('ascii'), 0) > 1
            if link['repeated']:
                link['binding_admitted'] = False
                link['issues'].append('repeated_singleton_source_field')
        for tag in ('DATA', 'DNAM', 'CRDT') if entry['kind_name'] == 'WEAP' else ('DATA',):
            if not occurrences[tag]:
                node['issues'].append(dict(field_index=None, field_kind=list(tag.encode()), code='required_attack_field_unavailable'))
        require(counts['field_visits'] <= 200000, 'field visit budget')
        return node

    nodes, selected, choice = [decode(weapon, 0)], {weapon: 0}, None
    if ammo is not None:
        provenance = source(ammo)
        status = 'missing' if provenance is None else 'deleted' if provenance['header']['flags'] & 0x20 else 'defined'
        allowed = None if provenance is None else provenance['header']['kind'] == list(b'AMMO')
        node_index = None
        if status == 'defined' and allowed is True:
            node_index = len(nodes)
            selected[ammo] = node_index
            nodes.append(decode(ammo, 1))
        counts['field_visits'] += len(nodes[0]['links'])
        relations = []
        for index, link in enumerate(nodes[0]['links']):
            if link['role'] == 'weapon_ammo':
                matched = key_tuple(link['binding']['key']) == ammo if link['binding_admitted'] and link['binding']['target']['kind'] == list(b'AMMO') and status == 'defined' and allowed is True else None
                relations.append(dict(weapon_link_index=index, matches_direct_ammo=matched, list_membership_verified=False))
        choice = dict(key=key_json(ammo), status=status, schema_kind_allowed=allowed, source=provenance, node_index=node_index, declared_relations=relations)
    for parent in range(len(nodes)):
        counts['field_visits'] += len(nodes[parent]['links'])
        for link in nodes[parent]['links']:
            if not link['binding_admitted'] or link['role'] not in ('weapon_projectile', 'ammo_projectile'):
                continue
            depth = 1 if parent == 0 else 2
            counts['source_depth'] = max(depth, counts['source_depth'])
            key = key_tuple(link['binding']['key'])
            if key not in selected:
                selected[key] = len(nodes)
                nodes.append(decode(key, depth))
            link['target_node'] = selected[key]
    result = dict(sources=native['sources'], winning_content_sha256=native['winning_content_sha256'], weapon=key_json(weapon),
        explicit_ammo=choice, source_nodes=nodes, counts=counts, ammo_choice_verified=False, projectile_priority_selected=False, firing_supported=False, scope=SCOPE)
    require(len(native['sources']) <= 256 and counts['field_visits'] <= 200000 and len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode()) <= 16 * MIB, 'source/output budgets')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--weapon', required=True)
    parser.add_argument('--ammo')
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
        require(control['mode'] == 'active' and not control['stop_requested'] and assignment['state'] == 'active' and assignment['implementation_authorized']
            and control['run_id'] == assignment['run_id'] == lease['run_id'] and control['generation'] == assignment['generation'] == lease['generation']
            and lease['session_uuid'] == args.session_id, 'actor attack authorization changed')
        for path in team.glob('*.outbox.jsonl'):
            for line in path.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != control['run_id'] or row.get('type') != 'stop_requested', 'STOP')
    def key(text):
        origin, local = text.rsplit(':', 1)
        return origin.lower(), int(local, 16)
    guard()
    require(args.base_report.stat().st_size <= 256 * MIB, 'native byte budget')
    native = json.loads(args.base_report.read_bytes())
    reader = Reader(args.data, json.loads(args.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = manifest(reader, native, key(args.weapon), None if args.ammo is None else key(args.ammo))
        if args.observed:
            require(args.observed.stat().st_size <= 256 * MIB, 'observed byte budget')
            observed = json.loads(args.observed.read_bytes())
            observed = observed['actor_attack_inputs']['manifest'] if 'actor_attack_inputs' in observed else observed
            require(result == observed, 'complete independent attack source projection differs')
        native['actor_attack_inputs'] = dict(manifest=result)
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(native, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True if args.observed else None, weapon=args.weapon, ammo=args.ammo,
            nodes=result['counts']['nodes'], headers=result['counts']['headers'], retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
