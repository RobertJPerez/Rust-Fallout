"""Authored exact PACK/navigation join oracle, using literal fixture source inputs.

Composes the existing protected Reader and PACK destination oracle. Navigation
expectations are authored independently before production decode and re-encoded
to the original bytes; no second navigation/plugin/asset decoder is introduced.
"""
import argparse
import hashlib
import heapq
import json
import math
import pathlib
import struct
from dependencies import MIB, Reader, fields, key_tuple, require
from package_destinations import manifest

SCOPE = 'Exact physical PACK operand joined to explicit caller-selected cell/node/cost/link inputs through existing source navigation and RouteGraph; reference/object destination mapping, door traversal, eligibility, scheduling and movement unverified; engineering proposal only, no canonical mutation'
REFUSAL = dict(code='faithful_ai_execution_unverified', dependencies=['package_eligibility', 'destination_to_node_mapping', 'door_traversal', 'special_link_traversal', 'scheduling', 'canonical_movement'])


def identity(node):
    return key_tuple(node['mesh']), node['triangle']


def encode_literals(mesh):
    """Fixture encoder: typed independently authored values must equal source bytes."""
    pack = struct.pack
    values = {
        'NVER': pack('<I', mesh['version']['value']),
        'DATA': pack('<6I', mesh['cell_raw']['value'], len(mesh['vertices']), len(mesh['triangles']), len(mesh['edge_links']), len(mesh['cover_triangles']), len(mesh['door_links'])),
        'NVVX': b''.join(pack('<3f', *v) for v in mesh['vertices']),
        'NVTR': b''.join(pack('<3H3h2H', *t['vertices'], *t['edges'], t['flags'], t['cover_flags']) for t in mesh['triangles']),
        'NVEX': b''.join(pack('<IIH', e['link_type'], e['navmesh_raw'], e['triangle']) for e in mesh['edge_links']),
        'NVCA': b''.join(pack('<H', v) for v in mesh['cover_triangles']),
        'NVDP': b''.join(pack('<IH2B', e['door_raw'], e['triangle'], *e['unused']) for e in mesh['door_links']),
    }
    out = b''
    for field in mesh['fields']:
        tag = bytes(field['kind']).decode('ascii')
        raw = bytes(field['bytes'])
        require(field['decoded_offset'] == len(out), 'literal physical field offset')
        if tag in values:
            require(values.pop(tag) == raw, 'literal typed words differ from input bytes')
        out += tag.encode() + pack('<H', len(raw)) + raw
    require(not values, 'literal expected counted field missing')
    return out


def source_meshes(reader, spec, cells):
    result = []
    for cell in cells:
        selected = [m for m in spec['meshes'] if m['cell'] == cell]
        require(selected, 'authored selected cell unavailable')
        for source in sorted(selected, key=lambda m: key_tuple(m['key'])):
            entry = reader.winners[key_tuple(source['key'])]
            mesh = source['mesh']
            require(entry['kind_name'] == 'NAVM' and source['source_sha256'] == reader.sources[entry['source']].sha256, 'literal NAVM source cohort')
            require(mesh['header'] == reader.header(reader.sources[entry['source']].read(entry['offset'], 24), entry['offset']), 'literal NAVM header')
            body = reader.payload(key_tuple(source['key']), 64*MIB)
            require(encode_literals(mesh) == body and len(body) == mesh['decoded_bytes'] and hashlib.sha256(body).hexdigest() == mesh['decoded_sha256'], 'complete independently authored navigation source')
            for edge, target in zip(mesh['edge_links'], source['external_targets']):
                require(reader.resolve(entry['source'], edge['navmesh_raw']) == (None if target is None else key_tuple(target)), 'literal external target identity')
            for door, target in zip(mesh['door_links'], source['door_targets']):
                require(reader.resolve(entry['source'], door['door_raw']) == (None if target is None else key_tuple(target)), 'literal door target identity')
            result.append(source)
    return result


def graph(meshes):
    nodes, edges = [], []
    for source in sorted(meshes, key=lambda m: key_tuple(m['key'])):
        mesh = source['mesh']
        for index, triangle in enumerate(mesh['triangles']):
            vertices = [mesh['vertices'][v] for v in triangle['vertices']]
            doors = [dict(raw=door['door_raw'], reference=source['door_targets'][i], unused=door['unused']) for i, door in enumerate(mesh['door_links']) if door['triangle'] == index]
            nodes.append(dict(id=dict(mesh=source['key'], triangle=index), cell=source['cell'], source_sha256=source['source_sha256'], record_flags=mesh['header']['flags'], triangle_flags=triangle['flags'], cover_flags=triangle['cover_flags'], centroid=[sum(v[i] for v in vertices)/3 for i in range(3)], doors=doors))
            outgoing = []
            for edge_index, raw in enumerate(triangle['edges']):
                external = bool(triangle['flags'] & (1 << edge_index))
                if raw == -1 and not external:
                    continue
                require(raw >= 0, 'literal negative special edge')
                portal = [mesh['vertices'][triangle['vertices'][e]] for e in [edge_index, (edge_index+1)%3]]
                if external:
                    link = mesh['edge_links'][raw]
                    key = source['external_targets'][raw]
                    target = None if key is None else dict(mesh=key, triangle=link['triangle'])
                    kind = dict(kind='external', link_type=link['link_type'], navmesh_raw=link['navmesh_raw'])
                else:
                    target = dict(mesh=source['key'], triangle=raw)
                    kind = dict(kind='local')
                outgoing.append(dict(source=dict(mesh=source['key'], triangle=index), source_edge=edge_index, target=target, kind=kind, portal=portal))
            edges.append(outgoing)
    return nodes, edges


def decision(value):
    kind = value['kind']
    if kind == 'admit':
        require(set(value) == {'kind', 'cost_bits'}, 'strict admit decision')
        cost = struct.unpack('<d', struct.pack('<Q', value['cost_bits']))[0]
        require(math.isfinite(cost) and cost >= 0, 'finite nonnegative explicit cost')
        return cost, None
    require(set(value) == ({'kind'} if kind == 'reject' else {'kind', 'reason'}) and kind in {'reject', 'unavailable'}, 'strict explicit decision')
    if kind == 'unavailable':
        require(0 < len(value['reason'].encode()) <= 1024, 'explicit reason budget')
        return None, value['reason']
    return None, None


def route(nodes, edges, query):
    lookup = {identity(n['id']): i for i, n in enumerate(nodes)}
    start, goal = lookup[identity(query['start']['node'])], lookup[identity(query['goal']['node'])]
    require(nodes[start]['cell'] == query['start']['cell'] and nodes[goal]['cell'] == query['goal']['cell'], 'exact explicit endpoint cell')
    distances = {start: 0.0}
    previous, queue, missing, unavailable = {}, [(0.0, start)], [], []
    policy = query['policy']
    external = {e['link_type']: e['decision'] for e in policy['external']}
    require(len(external) == len(policy['external']), 'unique special policy')
    for choice in [policy['local'], policy['external_fallback']] + list(external.values()):
        decision(choice)
    while queue:
        cost, at = heapq.heappop(queue)
        if cost != distances[at]:
            continue
        if at == goal:
            ids, links = [], []
            while True:
                ids.append(nodes[at]['id'])
                if at not in previous:
                    break
                before, edge = previous[at]
                links.append(edges[before][edge])
                at = before
            return dict(outcome='found', nodes=ids[::-1], links=links[::-1], cost=cost)
        for edge_index, link in enumerate(edges[at]):
            target = None if link['target'] is None else lookup.get(identity(link['target']))
            door = nodes[at]['doors'] or (target is not None and nodes[target]['doors'])
            if door and policy['doors']['kind'] == 'reject':
                continue
            if door and policy['doors']['kind'] == 'unavailable':
                unavailable.append(dict(link=link, reason=policy['doors']['reason']))
                continue
            value = policy['local'] if link['kind']['kind'] == 'local' else external.get(link['kind']['link_type'], policy['external_fallback'])
            edge_cost, reason = decision(value)
            if reason is not None:
                unavailable.append(dict(link=link, reason=reason))
            elif edge_cost is not None:
                if target is None:
                    missing.append(link)
                elif cost + edge_cost < distances.get(target, math.inf):
                    distances[target] = cost + edge_cost
                    previous[target] = at, edge_index
                    heapq.heappush(queue, (cost + edge_cost, target))
    if unavailable:
        return dict(outcome='unsupported', links=unavailable, missing_neighbors=missing)
    if missing:
        return dict(outcome='missing_neighbors', links=missing)
    return dict(outcome='unreachable')


def expected(reader, native, spec, query, snapshot, reference):
    require(set(query) == {'package', 'source_sha256', 'record_file_offset', 'field_index', 'field_decoded_offset', 'destination', 'cells', 'start', 'goal', 'policy'}, 'strict query')
    destination = manifest(reader, native, key_tuple(query['package']))
    require(query['source_sha256'] == destination['package']['source']['sha256'] and query['record_file_offset'] == destination['package']['source']['record_file_offset'], 'exact package source request')
    matches = [i for i, operand in enumerate(destination['operands']) if operand['field_index'] == query['field_index'] and operand['field_decoded_offset'] == query['field_decoded_offset']]
    require(len(matches) == 1, 'exact physical selected operand')
    index = matches[0]
    operand = destination['operands'][index]
    caller = None
    if reference is not None:
        require(snapshot['schema_version'] == 4, 'strict current snapshot')
        refs = [r for r in snapshot['references'] if r['id'] == reference]
        states = [r['state'] for r in snapshot['reference_states'] if r['id'] == reference]
        require(len(refs) == len(states) == 1, 'explicit current caller component')
        caller = dict(campaign=snapshot['campaign'], catalogue_sha256=snapshot['catalogue_sha256'], revision=snapshot['state_revision'], reference=reference, authored=refs[0]['authored'], state=states[0])
    visits = destination['field_visits'] + len(native['sources']) + len(query['policy']['external'])
    result = dict(query=query, destination=destination, operand_index=index, caller=caller, meshes=[], start=None, goal=None, route=None, route_nodes=[], outcome='destination_unavailable', visits=visits, destination_node_mapping_verified=False, execution_supported=False, state_changed=False, refusal=REFUSAL, scope=SCOPE)
    if not operand['binding_admitted']:
        return result
    require(operand['binding']['key'] == query['destination'], 'exact explicit binding key')
    if operand['alternative'] == 'cell':
        require(query['destination'] == query['goal']['cell'], 'literal CELL endpoint')
    meshes = source_meshes(reader, spec, query['cells'])
    visits += len(query['cells'])*len(reader.winners) + len(meshes)**2
    for mesh in meshes:
        m = mesh['mesh']
        visits += 3*len(m['triangles']) + sum(len(m[n]) for n in ['vertices', 'triangles', 'edge_links', 'cover_triangles', 'door_links', 'fields'])
    nodes, edges = graph(meshes)
    # Default public producer bounds reserved before traversal, including all
    # possible special-policy comparisons, expansions and returned path nodes.
    visits += 300000*(1 + len(query['policy']['external']).bit_length()) + 2*len(nodes) + 100000 + 10000
    proposal = route(nodes, edges, query)
    lookup = {identity(n['id']): n for n in nodes}
    result.update(meshes=meshes, start=lookup[identity(query['start']['node'])], goal=lookup[identity(query['goal']['node'])], route=proposal, route_nodes=[lookup[identity(n)] for n in proposal.get('nodes', [])], outcome='engineering_proposal', visits=visits)
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['data', 'load-order', 'base-report', 'authored-inputs', 'query', 'snapshot', 'observed', 'output']:
        p.add_argument('--'+name, type=pathlib.Path, required=True)
    p.add_argument('--reference', type=int)
    p.add_argument('--team-directory', type=pathlib.Path)
    p.add_argument('--session-id')
    a = p.parse_args()
    def guard():
        if a.team_directory is None:
            return
        team = a.team_directory
        read = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
        c, s, l = read(team.parent/'team/control.json'), read(team/'actors.assignment.json'), read(team/'leases/actors.json')
        require(c['mode'] == 'active' and not c['stop_requested'] and s['state'] == 'active' and s['implementation_authorized'] and c['generation'] == s['generation'] == l['generation'] and c['run_id'] == s['run_id'] == l['run_id'] and l['session_uuid'] == a.session_id, 'route oracle authorization changed')
        for box in team.glob('*.outbox.jsonl'):
            for line in box.read_text(encoding='utf-8-sig').splitlines():
                row = json.loads(line)
                require(row.get('run_id') != c['run_id'] or row.get('type') != 'stop_requested', 'STOP')
    guard()
    for path, limit in [(a.base_report, 256*MIB), (a.authored_inputs, 32*MIB), (a.query, MIB), (a.snapshot, 32*MIB), (a.observed, 32*MIB)]:
        require(path.stat().st_size <= limit, 'input byte budget')
    native, spec, query, snapshot, observed = (json.loads(path.read_bytes()) for path in [a.base_report, a.authored_inputs, a.query, a.snapshot, a.observed])
    reader = Reader(a.data, json.loads(a.load_order.read_text(encoding='utf-8-sig')), native, guard)
    try:
        result = expected(reader, native, spec, query, snapshot, a.reference)
        require(result == observed.get('actor_package_route', observed), 'complete independent package route differs')
        guard()
        with a.output.open('x', encoding='utf-8') as target:
            json.dump(result, target, separators=(',', ':'))
        print(json.dumps(dict(equal=True, outcome=result['outcome'], route=None if result['route'] is None else result['route']['outcome'], execution_supported=False, state_changed=False, retail_parity_accepted=False)))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
