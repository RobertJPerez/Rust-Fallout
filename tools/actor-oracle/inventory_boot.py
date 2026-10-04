"""Independent private engineering boot expectation using existing inventory oracle.

Source definitions come from the separate C++ inventory producer; fresh original
headers and body hashes use the existing locked Reader. Snapshot/facts are caller
inputs. No production boot result supplies an expected field.
"""
import argparse
import copy
import hashlib
import json
import pathlib
from dependencies import Reader, MIB, key_tuple, require

SCOPE = 'Private engineering boot from explicit physical direct CNTO choices and explicit positive host counts/facts; source signed counts and COED declarations retained separately, no live partial mutation, template inheritance, leveled rolls, respawn, default ammo/equipment or faithful actor initialization'


def expected(reader, native, snapshot, root, owner, choices):
    require(snapshot['schema_version'] == 4 and not any(bank['owner'] == owner for bank in snapshot['inventory_banks']), 'current uninitialized owner inventory')
    refs = [r for r in snapshot['references'] if r['id'] == owner]
    require(len(refs) == 1, 'one explicit owner')
    definitions = [d for d in native['definitions'] if key_tuple(d['key']) == root]
    require(len(definitions) == 1, 'one source actor inventory definition')
    source = definitions[0]
    entry = reader.winners[root]
    require(entry['kind_name'] in {'NPC_', 'CREA'} and not source['deleted'] and not entry['flags'] & 0x20, 'live source actor')
    require(source['source']['decoded_record_sha256'] == hashlib.sha256(reader.payload(root, 64 * MIB)).hexdigest(), 'independent source body hash')
    require(source['source']['record_file_offset'] == entry['offset'] and source['source']['record_flags'] == entry['flags'], 'independent source header identity')
    require(0 < len(choices) <= 4096 and len({c['field_index'] for c in choices}) == len(choices), 'distinct explicit choices')
    candidate = copy.deepcopy(snapshot)
    bank = dict(owner=owner, items=[])
    candidate['inventory_banks'].append(bank)
    candidate['inventory_banks'].sort(key=lambda b: b['owner'])
    candidate['state_revision'] += 1
    mappings = []
    for choice in choices:
        index = choice['field_index']
        require(sum(i['cnto_field'] == index for i in source['items']) == 1, 'physical source index')
        field = source['fields'][index]
        value = field['value']
        require(field['kind'] == list(b'CNTO') and value['kind'] == 'item', 'source CNTO')
        binding = value['item']
        require(binding == reader.binding(entry['source'], binding['raw_form']), 'independent source binding')
        require(binding['status'] == 'defined' and value['schema_kind_allowed'] is True and bytes(binding['target']['kind']) not in {b'LVLI', b'LVLC', b'LVLN'}, 'direct permitted item')
        require(choice['facts']['base'] == binding['key'] and 0 < choice['host_count'] <= 0xFFFFFFFF, 'explicit positive base/count')
        require(choice['source_count_claim'] is None or choice['source_count_claim'] == value['count'], 'source signed claim')
        item = candidate['next_item']
        candidate['next_item'] += 1
        candidate['state_revision'] += 1
        bank['items'].append(dict(id=item, owner=owner, count=choice['host_count'], facts=copy.deepcopy(choice['facts'])))
        mappings.append(dict(source_field_index=index, source_signed_count=value['count'], explicit_host_count=choice['host_count'], item=item))
    compact = json.dumps(snapshot, separators=(',', ':'), ensure_ascii=False).encode('utf-8')
    return dict(source_definition=source, owner=owner, owner_authored=refs[0]['authored'], input_snapshot_sha256=hashlib.sha256(compact).hexdigest(),
        candidate_snapshot=candidate, mappings=mappings, faithful_initialization_supported=False, scope=SCOPE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'inventory-report', 'snapshot', 'choices', 'observed', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--root', required=True)
    parser.add_argument('--owner', required=True, type=int)
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
            and control['generation'] == assignment['generation'] and lease['session_uuid'] == args.session_id, 'boot authorization changed')
        for mailbox in team.glob('*.outbox.jsonl'):
            for line in mailbox.read_text(encoding='utf-8-sig').splitlines():
                r = json.loads(line)
                require(r.get('run_id') != control['run_id'] or r.get('type') != 'stop_requested', 'STOP')
    guard()
    for path, maximum in [(args.inventory_report, 256 * MIB), (args.snapshot, 32 * MIB), (args.choices, 32 * MIB), (args.observed, 256 * MIB)]:
        require(path.stat().st_size <= maximum, 'input byte budget')
    native = json.loads(args.inventory_report.read_bytes())
    snapshot = json.loads(args.snapshot.read_bytes())
    choices = json.loads(args.choices.read_bytes())
    observed = json.loads(args.observed.read_bytes())
    origin, local = args.root.split(':')
    reader = Reader(args.data, json.loads(args.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = expected(reader, native, snapshot, (origin.lower(), int(local, 16)), args.owner, choices)
        require(result == observed.get('actor_inventory_boot', observed), 'complete independent private boot differs')
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(result, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True, owner=args.owner, mappings=result['mappings'], retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
