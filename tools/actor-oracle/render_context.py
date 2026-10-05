"""Independent current placement/render join through the existing source oracles.

Snapshot facts are explicit expected inputs, never restored mutation authority.
Reuse the source/graph/archive/context decoders; no renderer or NIF conversion.
"""
import argparse
import json
import math
import pathlib
import struct
from context import expected as context_expected
from dependencies import Reader, render_manifest, key_tuple, require, MIB

SCOPE = 'Exact caller-selected source model occurrences joined to fresh canonical actor placement/base and existing saved pose/enable; admitted only with explicit enabled state/positive scale and existing render source admission; no implicit paths, authored-pose reset, equipment, transform conversion, NIF/GPU readiness or canonical mutation'


def expected(reader, native, snapshot, reference, occurrences, definitions=None, graph=None):
    require(len(occurrences) <= 1024 and len(reader.names) <= 256, 'selection/source budget')
    context = context_expected(reader, native, snapshot, reference)
    root = key_tuple(context['actor']['key'])
    if definitions is None:
        definitions, _ = reader.definitions()
    if graph is None:
        graph = reader.graph()
    limits = dict(nodes=65536, edges=1000000, field_visits=2000000, paths=100000, path_bytes=32*MIB, candidates=100000, candidate_bytes=32*MIB)
    manifest = reader.manifest(root, definitions, graph, limits)
    render = render_manifest(reader, root, definitions, manifest,
        dict(sources=4096, requests=16384, issues=16384, visits=2000000), native['winning_content_sha256'])
    index = {}
    for i, request in enumerate(render['requests']):
        path = manifest['paths'][request['manifest_path_index']]
        identity = key_tuple(path['source']), path['field_index'], path['field_byte_offset']
        require(identity not in index, 'ambiguous physical request identity')
        index[identity] = i
    selected, identity_bytes = [], 0
    names = {'source', 'source_sha256', 'record_file_offset', 'field_index', 'field_decoded_offset', 'field_byte_offset', 'role'}
    for occurrence in occurrences:
        require(set(occurrence) == names, 'strict occurrence input')
        key = key_tuple(occurrence['source'])
        identity_bytes += len(key[0].encode()) + len(occurrence['source_sha256'].encode())
        require(identity_bytes <= MIB, 'identity byte budget')
        digest = occurrence['source_sha256']
        require(len(digest) == 64 and all(c in '0123456789abcdef' for c in digest), 'source digest shape')
        require((key, occurrence['field_index'], occurrence['field_byte_offset']) in index, 'selected occurrence unavailable')
        i = index[key, occurrence['field_index'], occurrence['field_byte_offset']]
        request, source = render['requests'][i], definitions[key]
        path = manifest['paths'][request['manifest_path_index']]
        require(source['source']['sha256'] == digest and source['header']['offset'] == occurrence['record_file_offset']
            and path['field_decoded_offset'] == occurrence['field_decoded_offset'] and request['role'] == occurrence['role'], 'selected source field/frame/role differs')
        selected.append(dict(render_request_index=i, manifest_path_index=request['manifest_path_index'], admitted=False))
    state = context['reference']['state']
    if state is None:
        outcome = 'component_unavailable'
    elif not state['enabled']:
        outcome = 'disabled'
    elif state['pose']['scale_bits'] is None:
        outcome = 'scale_unavailable'
    elif not render['selected_requests_admitted']:
        outcome = 'source_unavailable'
    elif not selected:
        outcome = 'no_selection'
    elif any(manifest['paths'][s['manifest_path_index']]['role'] != 'model' for s in selected):
        outcome = 'selected_path_is_not_mesh'
    else:
        scale = struct.unpack('<f', struct.pack('<I', state['pose']['scale_bits']))[0]
        require(math.isfinite(scale) and scale > 0, 'canonical scale')
        outcome = 'admitted'
    for item in selected:
        item['admitted'] = outcome == 'admitted'
    visits = len(reader.names) + len(occurrences) + context['visits'] + render['visits'] + len(render['sources']) + len(render['requests']) + 3*len(selected)
    require(visits <= 2000000, 'visit budget')
    result = dict(context=context, render=render, selected=selected, outcome=outcome, visits=visits, identity_bytes=identity_bytes,
        gpu_ready=False, state_changed=False, scope=SCOPE)
    require(len(json.dumps(result, separators=(',', ':'), ensure_ascii=False).encode()) <= 32*MIB, 'projection budget')
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'snapshot', 'selection', 'observed', 'output']:
        p.add_argument('--'+name, type=pathlib.Path, required=True)
    p.add_argument('--reference', type=int, required=True)
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
            and l['session_uuid'] == a.session_id, 'render context oracle authorization changed')
        for box in team.glob('*.outbox.jsonl'):
            for line in box.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != c['run_id'] or row.get('type') != 'stop_requested', 'STOP')
    guard()
    for path, maximum in [(a.base_report, 256*MIB), (a.snapshot, 32*MIB), (a.selection, 8*MIB), (a.observed, 32*MIB)]:
        require(path.stat().st_size <= maximum, 'input byte budget')
    native, snapshot, selected, observed = (json.loads(path.read_bytes()) for path in [a.base_report, a.snapshot, a.selection, a.observed])
    reader = Reader(a.data, json.loads(a.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        reader.archives_index()
        result = expected(reader, native, snapshot, a.reference, selected)
        require(result == observed.get('actor_render_context', observed), 'complete independent current render context differs')
        if 'actor_context' in observed:
            require(observed['actor_context'] == result['context'], 'standalone actor context differs')
        guard()
        with a.output.open('x', encoding='utf-8') as target:
            json.dump(result, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True, outcome=result['outcome'], selected=len(result['selected']), admitted=sum(s['admitted'] for s in result['selected']),
            reference=a.reference, gpu_ready=False, state_changed=False, retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
