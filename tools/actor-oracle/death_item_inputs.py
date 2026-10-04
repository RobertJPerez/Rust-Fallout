"""Independent selected LVLI requests over locked bytes and native actor sources.

Reuses the existing plugin/header/FormID reader. No item/owner body is read and
no draw, level comparison, death event or template inheritance is evaluated.
"""
import argparse
import collections
import hashlib
import json
import pathlib
import struct
from dependencies import Reader, MIB, fields, key_tuple, require, key_json
from context import physical

SCOPE = 'One physical actor INAM with bounded existing LVLI declaration candidates, unsigned u16 level/count bits and explicit absent counts, raw chance/flags/COED and exact winning headers. Traits inheritance and repeated singleton ambiguity remain explicit. No RNG, probability, level threshold, respawn, death event, item creation or runtime inventory mutation'
HEADER = ('kind', 'offset', 'stored_size', 'flags', 'form_id', 'revision', 'version', 'trailing_bytes')
ITEMS = {'ALCH', 'AMMO', 'ARMO', 'BOOK', 'CCRD', 'CHIP', 'CMNY', 'IMOD', 'KEYM', 'LVLI', 'MISC', 'NOTE', 'WEAP'}


def source(reader, key):
    h = reader.winners[key]
    s = reader.sources[h['source']]
    return dict(key=key_json(key), source_name=reader.names[h['source']], source_bytes=s.size,
        source_sha256=s.sha256, header={name: h[name] for name in HEADER})


def manifest(reader, native, inventory, root, field_index):
    actors = [a for a in native['definitions'] if key_tuple(a['key']) == root]
    inventories = [d for d in inventory['definitions'] if key_tuple(d['key']) == root]
    require(len(actors) == len(inventories) == 1, 'one exact actor/inventory declaration')
    actor, inv = actors[0], inventories[0]
    h = reader.winners[root]
    require(not actor['deleted'] and h['kind_name'] in ('NPC_', 'CREA'), 'live actor root')
    require(actor['source'] == inv['source'] and actor['source']['plugin'] == reader.names[h['source']]
        and actor['source']['sha256'] == reader.sources[h['source']].sha256
        and actor['source']['record_file_offset'] == h['offset'] and actor['source']['record_flags'] == h['flags'], 'exact actor source header')
    actor_fields = physical(reader, root, actor)
    associations = next(a for a in native['actor_associations']['definitions'] if key_tuple(a['key']) == root)['associations']
    chosen = [a for a in associations if a['field_index'] == field_index]
    require(len(chosen) == 1 and chosen[0]['role'] == 'death_item', 'physical death-item association')
    association = chosen[0]
    tag, _, raw = actor_fields[field_index]
    require(tag == 'INAM' and len(raw) == 4, 'physical INAM bytes')
    binding = reader.binding(h['source'], struct.unpack('<I', raw)[0])
    require(binding == association['binding'], 'fresh death-item binding')
    configs = [f for f in inv['fields'] if f['kind'] == list(b'ACBS')]
    mask = configs[0]['value']['template_flags'] if len(configs) == 1 and configs[0]['value']['kind'] == 'actor_base' else None
    template = None if mask is None else bool(mask & 1)
    repeated = sum(a['role'] == 'death_item' for a in associations) > 1
    c = dict(lists=0, source_depth=0, field_visits=2*len(associations)+len(actor['fields'])+len(inv['fields']),
        fields=len(actor['fields'])+len(configs), entries=0, bindings=0, headers=1,
        decoded_bytes=len(reader.payload(root, MIB)), raw_bytes=4)

    def bound(b):
        c['bindings'] += 1
        k = key_tuple(b['key'])
        if k not in reader.winners:
            return None
        c['headers'] += 1
        s = source(reader, k)
        t = b['target']
        require(t and t['kind'] == s['header']['kind'] and t['source_plugin'] == s['source_name']
            and t['record_file_offset'] == s['header']['offset'] and t['record_flags'] == s['header']['flags'], 'exact bound source header')
        return s

    request = bound(binding)
    if request:
        c['source_depth'] = 1
    allowed = association['schema_kind_allowed']
    issues = []
    if template is None:
        issues.append('unique_configuration_unavailable')
    if template:
        issues.append('traits_template_inheritance_unsupported')
    if repeated:
        issues.append('repeated_death_item_association')
    if binding['status'] != 'defined':
        issues.append({'null': 'null_death_item_association', 'missing': 'missing_death_item_target', 'deleted': 'deleted_death_item_target'}[binding['status']])
    if allowed is False:
        issues.append('death_item_target_kind_not_allowed')

    def node(key):
        reader.guard()
        require(c['lists'] < 4096 and reader.winners[key]['kind_name'] == 'LVLI', 'list budget/kind')
        c['lists'] += 1
        body = reader.payload(key, MIB)
        c['decoded_bytes'] += len(body)
        winner = reader.winners[key]
        supported = winner['version'] == 15
        n = dict(source=source(reader, key), decoded_record_sha256=hashlib.sha256(body).hexdigest(),
            record_version_supported=supported, fields=[], entries=[], links=[], chance_field_indices=[], flag_field_indices=[],
            global_field_indices=[], metadata_singletons_unambiguous=False, findings=[], issues=[])
        if not supported:
            n['issues'].append('unsupported_list_record_version')
        pending = None
        all_entries = []
        counts = collections.Counter()
        for index, (tag, offset, raw) in enumerate(fields(body)):
            reader.guard()
            c['fields'] += 1
            c['field_visits'] += 2
            c['raw_bytes'] += len(raw)
            value = dict(kind='opaque')
            typed_links = []
            if tag == 'LVLO':
                require(len(raw) in (8, 10, 12), 'LVLO width')
                level, padding, form = struct.unpack_from('<HHI', raw)
                b = reader.binding(winner['source'], form)
                allow = None if b['target'] is None else bytes(b['target']['kind']).decode('ascii') in ITEMS
                value = dict(kind='entry', level_bits=level, level_padding=padding, item=b,
                    count_bits=None if len(raw) < 10 else struct.unpack_from('<H', raw, 8)[0],
                    count_padding=None if len(raw) < 12 else struct.unpack_from('<H', raw, 10)[0], schema_kind_allowed=allow)
                pending = len(all_entries)
                all_entries.append(dict(lvlo_field=index, coed_fields=[], item_link=None, unique_extra_available=True))
                typed_links.append(('item', b, allow))
            elif tag == 'COED':
                require(len(raw) == 12, 'COED width')
                owner, word, condition = struct.unpack('<III', raw)
                b = reader.binding(winner['source'], owner)
                if b['status'] == 'null':
                    union = dict(kind='unused', raw_word=word)
                elif b['status'] != 'defined':
                    union = dict(kind='unresolved_owner', raw_word=word)
                elif bytes(b['target']['kind']) == b'NPC_':
                    union = dict(kind='global', binding=reader.binding(winner['source'], word))
                elif bytes(b['target']['kind']) == b'FACT':
                    union = dict(kind='required_rank', raw_word=word, value=struct.unpack_from('<i', raw, 4)[0])
                else:
                    union = dict(kind='unresolved_owner', raw_word=word)
                value = dict(kind='extra', owner=b, union_word=union, condition_bits=condition)
                if pending is None:
                    n['findings'].append(dict(field_decoded_offset=offset, code='orphan_entry_extra_field'))
                else:
                    e = all_entries[pending]
                    if e['coed_fields']:
                        n['findings'].append(dict(field_decoded_offset=offset, code='multiple_entry_extra_fields'))
                    e['coed_fields'].append(index)
                typed_links.append(('owner', b, None if b['target'] is None else bytes(b['target']['kind']) in (b'NPC_', b'FACT')))
                if union['kind'] == 'global':
                    b = union['binding']
                    typed_links.append(('extra_global', b, None if b['target'] is None else bytes(b['target']['kind']) == b'GLOB'))
            elif tag in ('LVLD', 'LVLF', 'LVLG'):
                require(len(raw) == (4 if tag == 'LVLG' else 1), 'list metadata width')
                pending = None
                counts[tag] += 1
                if counts[tag] > 1:
                    n['findings'].append(dict(field_decoded_offset=offset, code={'LVLD':'multiple_chance_none_fields','LVLF':'multiple_list_flag_fields','LVLG':'multiple_chance_global_fields'}[tag]))
                if tag == 'LVLD':
                    value = dict(kind='chance_none', raw=raw[0])
                    if supported: n['chance_field_indices'].append(index)
                elif tag == 'LVLF':
                    value = dict(kind='flags', raw=raw[0], all_lower_levels=bool(raw[0]&1), each_count=bool(raw[0]&2), use_all=bool(raw[0]&4))
                    if supported: n['flag_field_indices'].append(index)
                else:
                    b = reader.binding(winner['source'], struct.unpack('<I', raw)[0])
                    value = dict(kind='global', global_=b)
                    value['global'] = value.pop('global_')
                    typed_links.append(('chance_global', b, None if b['target'] is None else bytes(b['target']['kind']) == b'GLOB'))
                    if supported: n['global_field_indices'].append(index)
            else:
                pending = None
            n['fields'].append(dict(kind=list(tag.encode('latin1')), decoded_offset=offset, raw_bytes=list(raw),
                sha256=hashlib.sha256(raw).hexdigest(), value=value if supported else None))
            if supported:
                for role, b, allow in typed_links:
                    if role == 'item': all_entries[-1]['item_link'] = len(n['links'])
                    n['links'].append(dict(field_index=index, role=role, binding=b, source=bound(b),
                        schema_kind_allowed=allow, structural_binding_available=b['status']=='defined' and allow is True, nested_node=None))
        if supported:
            n['entries'] = all_entries
            for e in n['entries']:
                e['unique_extra_available'] = len(e['coed_fields']) <= 1
            c['entries'] += len(all_entries)
            c['field_visits'] += len(n['links']) + len(all_entries)
            n['metadata_singletons_unambiguous'] = len(n['chance_field_indices'])==len(n['flag_field_indices'])==1 and len(n['global_field_indices'])<=1
            if not n['metadata_singletons_unambiguous']: n['issues'].append('unique_list_metadata_unavailable')
        return n

    nodes, root_node = [], None
    if binding['status'] == 'defined' and allowed is True:
        root_node = 0
        k = key_tuple(binding['key'])
        indices = {k: 0}
        nodes.append(node(k))
        cursor = 0
        while cursor < len(nodes):
            for l in nodes[cursor]['links']:
                c['field_visits'] += 1
                if l['role'] == 'item' and l['structural_binding_available'] and l['source']['header']['kind'] == list(b'LVLI'):
                    k = key_tuple(l['binding']['key'])
                    if k not in indices:
                        indices[k] = len(nodes)
                        nodes.append(node(k))
                    l['nested_node'] = indices[k]
            cursor += 1
        children = [[l['nested_node'] for l in n['links'] if l['nested_node'] is not None] for n in nodes]
        c['field_visits'] += 6*(len(nodes)+sum(map(len, children)))
        incoming = [0]*len(nodes)
        for targets in children:
            for to in targets: incoming[to] += 1
        queue = collections.deque(i for i,n in enumerate(incoming) if n == 0)
        depths, processed = [0]*len(nodes), 0
        depths[0] = 1
        while queue:
            at = queue.popleft()
            processed += 1
            c['field_visits'] += 1
            for l in nodes[at]['links']:
                c['field_visits'] += 1
                if l['source']:
                    c['source_depth'] = max(c['source_depth'], depths[at]+1)
                    require(c['source_depth'] <= 32, 'depth budget')
                to = l['nested_node']
                if to is not None:
                    depths[to] = max(depths[to], depths[at]+1)
                    incoming[to] -= 1
                    if not incoming[to]: queue.append(to)
        require(processed == len(nodes), 'selected list cycle')
    require(c['decoded_bytes']<=8*MIB and c['field_visits']<=200000 and c['fields']<=65536 and c['entries']<=16384
        and c['bindings']<=65536 and c['headers']<=65536 and c['raw_bytes']<=MIB and len(native['sources'])<=256, 'request budgets')
    result = dict(sources=native['sources'], winning_content_sha256=native['winning_content_sha256'], actor=actor,
        configuration_fields=configs, traits_template_flag=template, association=association, actor_field=actor['fields'][field_index],
        actor_raw_bytes=list(raw), singleton_repeated=repeated, declaration_binding_available=template is False and not repeated
        and binding['status']=='defined' and allowed is True, death_item_source=request, root_node=root_node, nodes=nodes,
        counts=c, issues=issues, items_created=False, death_event_verified=False, roll_supported=False, scope=SCOPE)
    require(len(json.dumps(result, separators=(',', ':')).encode())<=16*MIB, 'projection budget')
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['data','load-order','base-report','inventory-report','output']:
        p.add_argument('--'+name,type=pathlib.Path,required=True)
    p.add_argument('--root',required=True)
    p.add_argument('--field-index',type=int,required=True)
    p.add_argument('--observed',type=pathlib.Path)
    p.add_argument('--team-directory',type=pathlib.Path)
    p.add_argument('--session-id')
    a = p.parse_args()
    def guard():
        if a.team_directory is None: return
        team=a.team_directory
        read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
        c,x,l=read(team.parent/'team/control.json'),read(team/'actors.assignment.json'),read(team/'leases/actors.json')
        require(c['mode']=='active' and not c['stop_requested'] and x['state']=='active' and x['implementation_authorized']
            and c['run_id']==x['run_id']==l['run_id'] and c['generation']==x['generation']==l['generation'] and l['session_uuid']==a.session_id,'actor authorization changed')
        for box in team.glob('*.outbox.jsonl'):
            for line in box.read_text(encoding='utf-8-sig').splitlines():
                r=json.loads(line)
                require(r.get('run_id')!=c['run_id'] or r.get('type')!='stop_requested','STOP')
    guard()
    require(a.base_report.stat().st_size<=256*MIB and a.inventory_report.stat().st_size<=256*MIB,'native input budget')
    native,inv=json.loads(a.base_report.read_bytes()),json.loads(a.inventory_report.read_bytes())
    require(native['sources']==inv['sources'] and native['winning_content_sha256']==inv['metadata']['winning_definitions_sha256'],'native source cohort')
    name,id_=a.root.rsplit(':',1)
    reader=Reader(a.data,json.loads(a.load_order.read_text(encoding='utf-8-sig')),native,guard)
    try:
        m=manifest(reader,native,inv,(name.lower(),int(id_,16)),a.field_index)
        if a.observed:
            require(a.observed.stat().st_size<=256*MIB,'observed input budget')
            observed=json.loads(a.observed.read_bytes())
            observed=observed['actor_death_item_inputs']['manifest'] if 'actor_death_item_inputs' in observed else observed
            require(observed==m,'complete independent death-item source projection differs')
        native['actor_death_item_inputs']=dict(manifest=m)
        with a.output.open('x',encoding='utf-8') as f: json.dump(native,f,separators=(',',':'))
        print(json.dumps(dict(equal=True if a.observed else None,root=a.root,field_index=a.field_index,counts=m['counts'],retail_parity_accepted=False)))
    finally: reader.close()


if __name__=='__main__': main()
