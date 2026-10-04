"""Join tested actor-root source paths to verified member and inspector receipts.

This offline matrix preserves structural candidates. Part/clip choice, relative
base resolution, poses and retail behavior remain separate requirements.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path

from coverage_matrix import catalogue, digest, load
from process_guard import guard

MIB = 1024 * 1024

def require(condition, reason):
    if not condition: raise ValueError(reason)

def exact(left, right):
    # Preserve JSON primitive types: Python's ordinary equality equates bools
    # with integers. Hash incremental canonical tokens without another tree.
    def canonical(value):
        result = hashlib.sha256()
        for token in json.JSONEncoder(sort_keys=True, separators=(',', ':')).iterencode(value):
            result.update(token.encode('utf-8'))
        return result.digest()
    return canonical(left) == canonical(right)

def receipt(path, expected, bound):
    require(path.stat().st_size <= bound, 'source receipt exceeds byte admission bound')
    require(path.stat().st_size == expected['bytes'] and digest(path) == expected['sha256'], 'source receipt bytes/hash differ')

def key(candidate):
    require(type(candidate['entry_index']) is int and 0 <= candidate['entry_index'] <= 0xffffffff, 'invalid source entry index')
    raw = candidate['original_path']
    require(isinstance(raw, list) and len(raw) <= 4096 and all(type(b) is int and 0 <= b <= 255 for b in raw), 'invalid source path bytes')
    return candidate['container'], candidate['entry_index'], bytes(raw)

def verify_projection(projection):
    require(projection.get('schema_version') == 1 and projection.get('runtime_ready') is False and projection.get('retail_behavior_verified') is False, 'actor projection scope/readiness differs')
    handoff_path = Path(projection['actor_handoff']['path'])
    receipt(handoff_path, projection['actor_handoff'], MIB)
    handoff = load(handoff_path, MIB)
    require(handoff.get('task_id') == 'ACT-07' and handoff.get('state') == 'tested_handoff' and handoff['commit_revision'] == projection['actor_commit'], 'tested actor handoff/commit differs')
    require(handoff['commit_code_binding']['all_tested_code_tool_test_hashes_match_commit'] is True, 'actor handoff code binding missing')
    source = projection['source_report']
    route = next((row for row in handoff['independent_comparisons'] if row['receipt'] == projection['comparison_receipt']), None)
    require(route is not None, 'actor comparison receipt not in tested handoff')
    comparison_path = Path(route['receipt']['path'])
    receipt(comparison_path, route['receipt'], MIB)
    comparison = load(comparison_path, MIB)
    require(exact(comparison, route['details']) and comparison['source_and_binaries_unchanged'] is True, 'actor comparison source binding differs')
    require(any(row['rust_report_sha256'] == source['sha256'] for row in comparison['phases']), 'actor source report not in tested comparison')
    source_path = Path(source['path'])
    receipt(source_path, source, 512 * MIB)
    require(set(projection['selected_ranges']) == {'manifests', 'independent_comparison'}, 'actor selected source ranges differ')
    with source_path.open('rb') as original:
        for name, expected in projection['selected_ranges'].items():
            count, offset = expected['bytes'], expected['offset']
            require(type(count) is int and type(offset) is int and 0 <= count <= 32 * MIB and 0 <= offset <= source['bytes'] and count <= source['bytes'] - offset, 'actor subtree span exceeds bound')
            original.seek(offset); raw = original.read(count)
            require(hashlib.sha256(raw).hexdigest() == expected['sha256'], 'actor subtree source hash differs')
            value = json.loads(raw)
            require(exact(value, projection[name]), 'actor subtree ordered fields differ')
    require(projection['independent_comparison']['equal'] is True, 'actor source independent comparison did not match')
    roots = projection['manifests']
    require(isinstance(roots, list) and len(roots) <= 64 and sum(len(row['paths']) for row in roots) <= 512, 'actor root/path count exceeds bound')
    require(sum(len(path['candidates']) for row in roots for path in row['paths']) <= 512, 'actor candidate count exceeds bound')
    require(exact([(row['root'], row['counts']) for row in roots], [(row['root'], row['counts']) for row in handoff['counts']['original']['manifests']]), 'actor root/count projection differs from handoff')
    identities = [json.dumps(row['root'], sort_keys=True) for row in roots]
    require(len(set(identities)) == len(identities), 'duplicate actor root')
    return roots

def build(projection, manifest, inputs, animation, skin, member_receipts):
    roots = verify_projection(projection)
    require(manifest.get('schema_version') == 1 and manifest.get('runtime_ready') is False and manifest.get('retail_behavior_verified') is False, 'member source scope/readiness differs')
    members = manifest.get('members')
    require(isinstance(members, list) and len(members) <= 128, 'member count exceeds bound')
    require(sum(row['decoded_bytes'] for row in members) <= 256 * MIB and all(0 <= row['decoded_bytes'] <= 64 * MIB for row in members), 'decoded member bytes exceed bound')
    archive = manifest['archive']
    archive_path = Path(archive['path'])
    receipt(archive_path, archive, 8 * 1024 * MIB)
    require(animation.get('schema_version') == 6 and skin.get('schema_version') == 3, 'actor matrix requires animation schema6 and skin schema3')
    animations, skins = catalogue(animation), catalogue(skin)
    selected = {}
    occurrences = {}
    for root in roots:
        for index, path in enumerate(root['paths']):
            status = path['lookup_status']
            require(status != 'one_archive_candidate' or len(path['candidates']) == 1, 'one-candidate status/count differs')
            for candidate in path['candidates']:
                identity = key(candidate)
                if not identity[2].lower().endswith(b'.nif'): continue
                selected[identity] = candidate
                occurrences.setdefault(identity, []).append(dict(root=root['root'], path_index=index))
    require(len(selected) <= 128, 'actor NIF candidate count exceeds bound')
    seen, files, products = set(), set(), []
    for member in members:
        guard()
        identity = key(member['candidate'])
        require(identity in selected and identity not in seen and exact(member['occurrences'], occurrences[identity]), 'member candidate tuple/order or actor occurrences differ')
        seen.add(identity)
        require(identity[0] == archive['path'], 'member archive source differs')
        name = member['file']
        require(Path(name).name == name and name not in files, 'duplicate or non-leaf member filename')
        files.add(name)
        payload = inputs / name
        receipt(payload, dict(bytes=member['decoded_bytes'], sha256=member['sha256']), 64 * MIB)
        backend = member['backend_comparison']
        backend_name = backend['receipt_file']
        require(Path(backend_name).name == backend_name, 'non-leaf member backend receipt')
        backend_path = member_receipts / backend_name
        require(backend_path.stat().st_size <= 16 * MIB and digest(backend_path) == backend['sha256'] and backend['same_decoded_digest_length_and_stored_offset'] is True, 'member backend comparison binding differs')
        decoded = load(backend_path, 16 * MIB)
        require(decoded['archive'] == identity[0] and decoded['path'].encode('ascii') == identity[2] and decoded['sha256'] == member['sha256'] and decoded['decoded_bytes'] == member['decoded_bytes'] and decoded['file_offset'] == member['stored_offset'], 'member backend identity/hash/length/offset differs')
        nif = decoded['nif']
        require([nif['version'], nif['user_version'], nif['bethesda_version']] == member['tuple'] and nif['block_counts'] == member['block_counts'], 'member backend container tuple/counts differ')
        source_offset, stored_bytes = member['stored_offset'], member['stored_bytes']
        require(0 <= stored_bytes <= 64 * MIB and 0 <= source_offset <= archive['bytes'] and stored_bytes <= archive['bytes'] - source_offset, 'member stored source span exceeds bound')
        with archive_path.open('rb') as source:
            source.seek(source_offset); raw = source.read(stored_bytes)
        require(hashlib.sha256(raw).hexdigest() == member['stored_sha256'], 'member stored source hash differs')
        rows = [animations.get(name), skins.get(name)]
        require(all(row is not None and row['sha256'] == member['sha256'] and row['decoded_bytes'] == member['decoded_bytes'] and row['tuple'] == member['tuple'] for row in rows), 'inspector member identity/tuple differs')
        anim, sk = rows
        require(anim['container_block_counts'] == member['block_counts'], 'animation container counts differ')
        counts = Counter()
        for section in ('animation', 'keys', 'splines', 'spline_components', 'bool_interpolators', 'bool_keys'):
            data = anim[section]
            require(data['runtime_ready'] is False and len(data['blocks']) <= 100000, 'animation catalogue readiness/count differs')
            counts.update(block['block_type'] for block in data['blocks'])
        require(all(count <= member['block_counts'].get(kind, 0) for kind, count in counts.items()), 'animation catalogue exceeds container counts')
        require(all(sk[section]['runtime_ready'] is False for section in ('skin', 'partitions', 'bindings')), 'skin catalogue readiness differs')
        products.append(dict(candidate=member['candidate'],file=name,sha256=member['sha256'],decoded_bytes=member['decoded_bytes'],tuple=member['tuple'],occurrences=member['occurrences'],animation_source_counts=dict(counts),skin_source=dict(blocks=len(sk['skin']['blocks']),owners=len(sk['skin']['owners']),partitions=len(sk['partitions']['blocks']),binding_nodes=len(sk['bindings']['nodes']),instances=len(sk['bindings']['instances']),binding_diagnostics=len(sk['bindings']['diagnostics']),unsupported_scene_edges=len(sk['bindings']['unsupported_scene_edges']),ancestry_scope=sk['bindings']['ancestry_scope']),runtime_ready=False,evaluated_pose=False,retail_behavior_verified=False))
    require(seen == set(selected) and files == set(animations) == set(skins), 'actor candidate/member/inspector sets differ')
    root_rows = []
    for root in roots:
        root_members = [row['file'] for row in products if any(item['root'] == root['root'] for item in row['occurrences'])]
        root_rows.append(dict(root=root['root'],source_counts=root['counts'],ordered_paths=root['paths'],verified_nif_members=root_members,lookup_statuses=dict(Counter(path['lookup_status'] for path in root['paths'])),part_selection_verified=False,relative_base_resolved=False))
    return dict(schema_version=1,scope='Exact tested actor structural candidates joined to source extraction and skin/animation comparison receipts; no part/clip choice',actor_commit=projection['actor_commit'],roots=root_rows,members=products,runtime_ready=False,evaluated_poses=False,retail_behavior_verified=False)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('actor_projection', 'member_manifest', 'member_receipts', 'inputs', 'animation_report', 'animation_oracle', 'skin_report', 'skin_oracle', 'output'):
        parser.add_argument('--' + name.replace('_', '-'), type=Path, required=True)
    args = parser.parse_args()
    guard()
    projection, manifest = load(args.actor_projection, 32 * MIB), load(args.member_manifest, 16 * MIB)
    require(digest(args.actor_projection) == manifest['actor_projection_sha256'], 'member manifest actor projection binding differs')
    animation, skin = load(args.animation_report, 128 * MIB), load(args.skin_report, 64 * MIB)
    for report, oracle in ((animation, args.animation_oracle), (skin, args.skin_oracle)):
        require(report['oracle_report_sha256'] == digest(oracle), 'inspector native comparison receipt binding differs')
    result = build(projection, manifest, args.inputs, animation, skin, args.member_receipts)
    result['evidence_sha256'] = {name:digest(getattr(args,name)) for name in ('actor_projection','member_manifest','animation_report','animation_oracle','skin_report','skin_oracle')}
    with args.output.open('x', encoding='utf-8') as output:json.dump(result, output, indent=2);output.write('\n')
    print(json.dumps(dict(roots=len(result['roots']),members=len(result['members']),skin_blocks=sum(row['skin_source']['blocks'] for row in result['members']),skin_owners=sum(row['skin_source']['owners'] for row in result['members']),binding_nodes=sum(row['skin_source']['binding_nodes'] for row in result['members']),runtime_ready=False)))

if __name__ == '__main__': main()
