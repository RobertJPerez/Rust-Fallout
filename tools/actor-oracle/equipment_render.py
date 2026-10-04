"""Independent canonical lot/model join through existing lot and model oracles.

Snapshot input supplies explicit canonical facts; it is not restored authority.
No equipped-state inference, model decoder, source parser or gameplay evaluation.
"""
import argparse
import json
import pathlib
from context import physical
from dependencies import Reader, key_tuple, require, MIB
from equipment import manifest as model_manifest
from equipment_lots import expected as lot_expected

SCOPE = 'Exact current canonical item handle/owner/lot joined to explicit caller source actor/model role; unknown/raw equipped slots and modifications do not choose equipped state, sex, role or mod mask; no actor-origin inference, equip mutation, attachment, texture swap or original behavior admission'


def expected(reader, native, snapshot, owner, item, actor, role):
    selected = lot_expected(reader, snapshot, owner, item)
    definition = next(d for d in native['definitions'] if key_tuple(d['key']) == actor)
    require(not definition['deleted'] and bytes(definition['kind']) in {b'NPC_', b'CREA'}, 'explicit actor source')
    physical(reader, actor, definition)
    base = key_tuple(selected['inventory']['items'][selected['selected_item_index']]['facts']['base'])
    model = model_manifest(reader, actor, base, role, native['winning_content_sha256'])
    require(model['actor']['source'] == definition['source'], 'native actor origin')
    result = dict(selection=selected, model=model, state_revision=snapshot['state_revision'],
        equipped_state_verified=False, actor_reference_bound=False, scope=SCOPE)
    require(len(model['sources']) <= 256 and len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode()) <= 32*MIB, 'joined projection budget')
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for n in ['data', 'load-order', 'base-report', 'snapshot', 'observed', 'output']:
        p.add_argument('--'+n, type=pathlib.Path, required=True)
    p.add_argument('--owner', type=int, required=True)
    p.add_argument('--item', type=int, required=True)
    p.add_argument('--actor', required=True)
    p.add_argument('--equipment-role', required=True)
    p.add_argument('--team-directory', type=pathlib.Path)
    p.add_argument('--session-id')
    a = p.parse_args()

    def guard():
        if a.team_directory is None:
            return
        team = a.team_directory
        read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
        c, s, l = read(team.parent/'team/control.json'), read(team/'actors.assignment.json'), read(team/'leases/actors.json')
        require(c['mode'] == 'active' and not c['stop_requested'] and s['state'] == 'active' and s['implementation_authorized']
            and c['generation'] == s['generation'] == l['generation'] and c['run_id'] == s['run_id'] == l['run_id']
            and l['session_uuid'] == a.session_id, 'joined oracle authorization changed')
        for box in team.glob('*.outbox.jsonl'):
            for line in box.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != c['run_id'] or row.get('type') != 'stop_requested', 'STOP')

    guard()
    for path, maximum in [(a.base_report, 256*MIB), (a.snapshot, 32*MIB), (a.observed, 32*MIB)]:
        require(path.stat().st_size <= maximum, 'input byte budget')
    native = json.loads(a.base_report.read_bytes())
    snapshot = json.loads(a.snapshot.read_bytes())
    observed = json.loads(a.observed.read_bytes())
    name, local = a.actor.rsplit(':', 1)
    local = int(local, 16)
    require(0 < local <= 0xffffff and a.owner > 0 and a.item > 0, 'explicit canonical identity')
    reader = Reader(a.data, json.loads(a.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        reader.archives_index()
        result = expected(reader, native, snapshot, a.owner, a.item, (name.lower(), local), a.equipment_role)
        require(result == observed.get('equipment_model', observed), 'complete independent lot/model join differs')
        if 'equipment_item' in observed:
            require(observed['equipment_item'] == result['selection'], 'standalone lot selection differs')
        guard()
        with a.output.open('x', encoding='utf-8') as target:
            json.dump(result, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True, owner=a.owner, item=a.item,
            selected_index=result['selection']['selected_item_index'], explicit_role=result['model']['explicit_choice']['role'],
            source_records=len(result['model']['source_records']), model_requests=len(result['model']['requests']),
            equipped_state_verified=False, retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
