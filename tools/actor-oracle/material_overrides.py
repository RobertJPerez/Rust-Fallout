"""Independent alternate-texture frames for an existing explicit equipment role.

Uses the established locked plugin/header/FormID and equipment readers. Raw
length-prefixed names remain bytes; TXST bodies, meshes and shaders are unread.
"""
import argparse
import collections
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, key_tuple, key_json, require
from equipment import manifest as equipment_manifest
from context import physical

SCOPE = 'Explicit existing equipment model role and physical alternate-texture arrays with raw length-prefixed mesh names, signed indices and winning TXST header requests. Exact unordered role mapping and duplicate ambiguity; no model/name fallback, mod palette inheritance, texture-set body import, archive precedence, mesh selection, shader/material mutation or equip/gameplay choice'
HEADER = ('kind', 'offset', 'stored_size', 'flags', 'form_id', 'revision', 'version', 'trailing_bytes')


def source(reader, key):
    h = reader.winners[key]
    s = reader.sources[h['source']]
    return dict(key=key_json(key), source_name=reader.names[h['source']], source_bytes=s.size,
        source_sha256=s.sha256, header={n: h[n] for n in HEADER})


def manifest(reader, native, actor, chosen, role):
    old = equipment_manifest(reader, actor, chosen, role, native['winning_content_sha256'])
    kind = old['explicit_choice']['role']
    pairs = {'armor_biped': {'male': ('MODL', 'MODS'), 'female': ('MOD3', 'MO3S')},
        'armor_world': {'male': ('MOD2', 'MO2S'), 'female': ('MOD4', 'MO4S')}}
    if kind['kind'] in pairs:
        pair = pairs[kind['kind']][kind['sex']]
    elif kind['kind'] == 'weapon_model':
        pair = None if kind['mod_mask'] else ('MODL', 'MODS')
    else:
        pair = {'weapon_shell': ('MOD2', 'MO2S'), 'weapon_scope': ('MOD3', 'MO3S'),
            'weapon_world': ('MOD4', 'MO4S'), 'weapon_first_person': ('MODL', 'MODS')}[kind['kind']]
    selected = None
    if kind['kind'] == 'weapon_first_person':
        if len(old['selected_links']) == 1 and not old['selected_links'][0]['ambiguous_source']:
            selected = old['selected_links'][0]['target_source_index']
    elif old['source_records'] and old['source_records'][0]['source']['decoded_record_sha256'] is not None:
        selected = 0
    c = dict(field_visits=old['counts']['visits'], fields=old['counts']['fields'], arrays=0, entries=0,
        name_bytes=0, raw_bytes=0, bindings=0, headers=0, decoded_bytes=old['counts']['decoded_bytes'])
    m = dict(equipment=old, selected_source_index=selected,
        selected_model_kind=None if pair is None else list(pair[0].encode()),
        alternate_kind=None if pair is None else list(pair[1].encode()), model_field_indices=[],
        selected_model_unique=False, alternate_array_repeated=False, arrays=[], counts=c, issues=[],
        texture_swaps_applied=False, mesh_target_selected=False, render_material_supported=False, scope=SCOPE)
    if pair is None: m['issues'].append('modded_model_alternate_role_unavailable')
    if selected is None: m['issues'].append('selected_model_source_unavailable')
    if selected is not None and pair is not None:
        definition = old['source_records'][selected]
        key = key_tuple(definition['key'])
        winner = reader.winners[key]
        require(winner['version'] == 15 and definition['header'] == source(reader, key)['header'], 'exact selected model header')
        raw_fields = physical(reader, key, definition)
        c['field_visits'] += len(raw_fields)
        m['model_field_indices'] = [i for i, (tag, _, _) in enumerate(raw_fields) if tag == pair[0]]
        m['selected_model_unique'] = len(m['model_field_indices']) == 1
        if not m['selected_model_unique']: m['issues'].append('unique_selected_model_field_unavailable')
        for index, (tag, offset, raw) in enumerate(raw_fields):
            reader.guard()
            c['field_visits'] += 1
            if tag != pair[1]: continue
            c['arrays'] += 1
            c['raw_bytes'] += len(raw)
            require(len(raw) >= 4, 'alternate count prefix')
            count = struct.unpack_from('<I', raw)[0]
            c['entries'] += count
            require(c['entries'] <= 4096 and 4 + 12*count <= len(raw), 'alternate count/extent budget')
            a = dict(source_index=selected, field_index=index, kind=list(tag.encode()), decoded_offset=offset,
                sha256=hashlib.sha256(raw).hexdigest(), raw_bytes=list(raw), declared_count=count, entries=[])
            cursor = 4
            for ordinal in range(count):
                c['field_visits'] += 1
                start = cursor
                require(cursor + 4 <= len(raw), 'mesh name prefix extent')
                length = struct.unpack_from('<I', raw, cursor)[0]
                name_at = cursor + 4
                cursor = name_at + length
                require(cursor + 8 <= len(raw), 'mesh name/texture/index extent')
                name = raw[name_at:cursor]
                c['name_bytes'] += len(name)
                require(c['name_bytes'] <= MIB, 'mesh name byte budget')
                texture_at, index_at = cursor, cursor + 4
                word, index_word = struct.unpack_from('<II', raw, cursor)
                mesh_index = struct.unpack_from('<i', raw, index_at)[0]
                cursor += 8
                b = reader.binding(winner['source'], word)
                c['bindings'] += 1
                texture_key = key_tuple(b['key'])
                target = source(reader, texture_key) if texture_key in reader.winners else None
                if target: c['headers'] += 1
                allowed = None if b['target'] is None else bytes(b['target']['kind']) == b'TXST'
                a['entries'].append(dict(ordinal=ordinal, field_byte_offset=start, name_byte_offset=name_at,
                    name_bytes=list(name), texture_byte_offset=texture_at, index_byte_offset=index_at,
                    index_word=index_word, mesh_index=mesh_index, texture=b, texture_source=target,
                    schema_kind_allowed=allowed, duplicate_mesh_declaration=False,
                    source_binding_available=m['selected_model_unique'] and b['status']=='defined' and allowed is True))
            require(cursor == len(raw), 'alternate trailing bytes')
            m['arrays'].append(a)
        m['alternate_array_repeated'] = len(m['arrays']) > 1
        if not m['arrays']: m['issues'].append('selected_alternate_array_absent')
        if m['alternate_array_repeated']: m['issues'].append('repeated_selected_alternate_array')
        repeated = collections.Counter((tuple(e['name_bytes']), e['mesh_index']) for a in m['arrays'] for e in a['entries'])
        c['field_visits'] += 3*c['entries']
        for a in m['arrays']:
            for e in a['entries']:
                e['duplicate_mesh_declaration'] = repeated[(tuple(e['name_bytes']), e['mesh_index'])] > 1
                if m['alternate_array_repeated'] or e['duplicate_mesh_declaration']: e['source_binding_available'] = False
    require(len(native['sources']) <= 256 and c['decoded_bytes'] <= 8*MIB and c['field_visits'] <= 200000
        and c['fields'] <= 65536 and c['arrays'] <= 256 and c['raw_bytes'] <= 2*MIB
        and c['bindings'] <= 4096 and c['headers'] <= 4096, 'material source budgets')
    require(all(reader.winners[key_tuple(d['key'])]['stored_size'] <= MIB
        and len(reader.payload(key_tuple(d['key']), MIB)) <= MIB for d in old['source_records']
        if d['source']['decoded_record_sha256'] is not None), 'equipment record budget')
    require(len(json.dumps(m, separators=(',', ':'), ensure_ascii=False).encode()) <= 16*MIB, 'projection budget')
    return m


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for n in ['data', 'load-order', 'base-report', 'output']: p.add_argument('--'+n, type=pathlib.Path, required=True)
    for n in ['actor', 'equipment-source', 'equipment-role']: p.add_argument('--'+n, required=True)
    p.add_argument('--observed', type=pathlib.Path)
    p.add_argument('--team-directory', type=pathlib.Path)
    p.add_argument('--session-id')
    a = p.parse_args()
    def guard():
        if a.team_directory is None: return
        team = a.team_directory
        read = lambda p: json.loads(p.read_text(encoding='utf-8-sig'))
        c, x, l = read(team.parent/'team/control.json'), read(team/'actors.assignment.json'), read(team/'leases/actors.json')
        require(c['mode']=='active' and not c['stop_requested'] and x['state']=='active' and x['implementation_authorized']
            and c['run_id']==x['run_id']==l['run_id'] and c['generation']==x['generation']==l['generation']
            and l['session_uuid']==a.session_id, 'actor authorization changed')
        for box in team.glob('*.outbox.jsonl'):
            for line in box.read_text(encoding='utf-8-sig').splitlines():
                r = json.loads(line)
                require(r.get('run_id')!=c['run_id'] or r.get('type')!='stop_requested', 'STOP')
    def key(text):
        name, local = text.rsplit(':', 1)
        local = int(local, 16)
        require(0 < local <= 0xffffff, 'canonical key')
        return name.lower(), local
    guard()
    require(a.base_report.stat().st_size <= 256*MIB, 'base input budget')
    native = json.loads(a.base_report.read_bytes())
    reader = Reader(a.data, json.loads(a.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        reader.archives_index()
        m = manifest(reader, native, key(a.actor), key(a.equipment_source), a.equipment_role)
        if 'actor_equipment_dependencies' in native:
            require(native['actor_equipment_dependencies']['manifest'] == m['equipment'], 'existing independent equipment differs')
        if a.observed:
            require(a.observed.stat().st_size <= 256*MIB, 'observed input budget')
            observed = json.loads(a.observed.read_bytes())
            observed = observed['actor_material_overrides']['manifest'] if 'actor_material_overrides' in observed else observed
            require(observed == m, 'complete independent material declaration differs')
        native['actor_equipment_dependencies'] = dict(manifest=m['equipment'])
        native['actor_material_overrides'] = dict(manifest=m)
        guard()
        with a.output.open('x', encoding='utf-8') as f: json.dump(native, f, separators=(',', ':'))
        print(json.dumps(dict(equal=True if a.observed else None, counts=m['counts'], retail_parity_accepted=False)))
    finally: reader.close()


if __name__ == '__main__': main()
