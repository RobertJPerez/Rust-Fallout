"""Independent explicit actor component, whole-lot and slot-metadata candidates.

Reuses the protected source Reader and ACT15/16 source/lot projections. Expected
changes derive only from the input snapshot and explicit request; no live runtime,
second plugin/script decoder, retail initialization or equip rules are used.
"""
import argparse
import copy
import hashlib
import json
import math
import pathlib
import struct
from dependencies import Reader, key_tuple, require, MIB
from context import expected as actor_context
from equipment_lots import expected as lot_selection

SCOPE = ('Explicit engineering intent applied once through canonical APIs to a privately restored candidate; '
         'source identity qualified, input unchanged, no retail initialization, AI, pickup, equip or slot conflict rules')


def canonical(value):
    """Order existing schema4 object fields only for its compact identity hash.

    This performs no save admission or restore and never supplies absent facts.
    It lets the oracle hash an actual CLI-extracted snapshot whose JSON map keys
    have a different presentation order than the canonical Rust struct fields.
    """
    if isinstance(value, list):
        return [canonical(v) for v in value]
    if not isinstance(value, dict):
        return value
    orders = [
        'schema_version campaign state_revision profile catalogue_sha256 next_item inventory_banks next_instance next_reference next_event_sequence clocks references instances pending_events reference_states',
        'profile origin_plugin local_id', 'owner items', 'id owner count facts',
        'base condition ownership equipped_slots ammo modifications quest_item script_instance extra_fields',
        'base count', 'tag bytes', 'tick game_nanoseconds menu_nanoseconds real_nanoseconds',
        'id authored', 'id state', 'schema_version cell pose enabled', 'position_bits rotation_bits scale_bits',
        'id definition owner context locals', 'key version_sha256', 'record header_decoded_offset',
        'calling_reference containing_reference target arguments', 'index value',
        'sequence instance trigger context arrived',
    ]
    keys = set(value)
    order = next((s.split() for s in orders if set(s.split()) == keys), None)
    if order is None and 'kind' in value:
        variants = ['kind', 'key', 'value', 'bits', 'rank', 'reference', 'id', 'activation', 'event_id', 'begin_byte_offset', 'mask']
        require(keys <= set(variants), 'known native tagged value for identity hash')
        order = [k for k in variants if k in value]
    require(order is not None, 'known existing native object for identity hash')
    return {k: canonical(value[k]) for k in order}


def digest(snapshot):
    return hashlib.sha256(json.dumps(canonical(snapshot), ensure_ascii=False, separators=(',', ':')).encode('utf-8')).hexdigest()


def expected(reader, native, snapshot, choice, operation):
    claim = choice['claim']
    require(claim['intent'] == {'kind': 'engineering'}, 'explicit engineering intent')
    require(digest(snapshot) == claim['expected_snapshot_sha256'], 'exact compact input snapshot identity')
    before = actor_context(reader, native, snapshot, claim['reference'])
    require(before['actor']['key'] == claim['actor'], 'exact selected actor base')
    candidate = copy.deepcopy(snapshot)
    selected = None
    visits = before['visits']
    delta = 1
    if operation == 'reference':
        entry = reader.winners[key_tuple(choice['cell'])]
        require(entry['kind_name'] == 'CELL' and not entry['flags'] & 0x20, 'live explicit cell')
        for bits in choice['position_bits'] + choice['rotation_bits']:
            require(math.isfinite(struct.unpack('<f', struct.pack('<I', bits))[0]), 'finite supplied pose')
        if choice['scale_bits'] is not None:
            scale = struct.unpack('<f', struct.pack('<I', choice['scale_bits']))[0]
            require(math.isfinite(scale) and scale > 0, 'positive supplied scale')
        state = dict(schema_version=1, cell=choice['cell'], pose=dict(position_bits=choice['position_bits'],
            rotation_bits=choice['rotation_bits'], scale_bits=choice['scale_bits']), enabled=choice['enabled'])
        candidate['reference_states'] = [s for s in candidate['reference_states'] if s['id'] != claim['reference']]
        candidate['reference_states'].append(dict(id=claim['reference'], state=state))
        candidate['reference_states'].sort(key=lambda s: s['id'])
        mutation = dict(kind='reference', state=state)
        visits += 4
    else:
        selected = lot_selection(reader, snapshot, claim['reference'], choice['item'])
        source_bank = next(b for b in candidate['inventory_banks'] if b['owner'] == claim['reference'])
        lot = next(i for i in source_bank['items'] if i['id'] == choice['item'])
        visits += selected['visits'] + 2
        if operation == 'transfer':
            require(any(r['id'] == choice['destination'] for r in snapshot['references']), 'registered destination')
            destination = [b for b in candidate['inventory_banks'] if b['owner'] == choice['destination']]
            require(len(destination) == 1, 'destination explicitly initialized')
            changed = claim['reference'] != choice['destination']
            delta = int(changed)
            if changed:
                source_bank['items'].remove(lot)
                lot['owner'] = choice['destination']
                destination[0]['items'].append(lot)
                destination[0]['items'].sort(key=lambda i: i['id'])
            mutation = dict(kind='transfer', item=choice['item'], destination=choice['destination'], changed=changed)
        else:
            require(operation == 'equipment', 'known operation')
            slots = choice['supplied_slots']
            require(slots is None or (all(0 <= s <= 65535 for s in slots) and len(slots) == len(set(slots))), 'unique explicit slots')
            lot['facts']['equipped_slots'] = slots
            facts = lot['facts']
            links = len(slots or []) + len(facts['modifications'] or []) + len(facts['extra_fields'])
            links += sum(facts[n] is not None for n in ['ammo', 'script_instance', 'ownership'])
            visits += links
            mutation = dict(kind='equipment', item=choice['item'], supplied_slots=slots)
    require(snapshot['state_revision'] <= (2**64 - 1) - delta, 'revision capacity')
    candidate['state_revision'] += delta
    return dict(claim=claim, actor_before=before, selected_before=selected, operation=mutation,
        input_snapshot_sha256=digest(snapshot), candidate_snapshot_sha256=digest(candidate),
        candidate_snapshot=candidate, visits=visits, faithful_rules_supported=False, scope=SCOPE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'snapshot', 'choice', 'observed', 'output']:
        parser.add_argument('--' + name, type=pathlib.Path, required=True)
    parser.add_argument('--operation', choices=['reference', 'transfer', 'equipment'], required=True)
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
            and control['run_id'] == assignment['run_id'] == lease['run_id']
            and control['generation'] == assignment['generation'] == lease['generation']
            and lease['session_uuid'] == args.session_id, 'oracle authorization changed')
        for path in team.glob('*.outbox.jsonl'):
            for line in path.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != control['run_id'] or row.get('type') != 'stop_requested', 'STOP')

    guard()
    for path, maximum in [(args.base_report, 256*MIB), (args.snapshot, 32*MIB), (args.choice, MIB), (args.observed, 64*MIB)]:
        require(path.stat().st_size <= maximum, 'input byte budget')
    native, snapshot, choice, observed = [json.loads(p.read_bytes()) for p in [args.base_report, args.snapshot, args.choice, args.observed]]
    reader = Reader(args.data, json.loads(args.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = expected(reader, native, snapshot, choice, args.operation)
        name = {'reference': 'actor_reference_intent', 'transfer': 'actor_inventory_transfer', 'equipment': 'actor_equipment_intent'}[args.operation]
        require(result == observed.get(name, observed), 'complete independent actor candidate differs')
        guard()
        with args.output.open('x', encoding='utf-8') as target:
            json.dump(result, target, ensure_ascii=False, separators=(',', ':'))
        print(json.dumps(dict(equal=True, operation=args.operation, visits=result['visits'], candidate_sha256=result['candidate_snapshot_sha256'], retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
