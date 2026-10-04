"""Join bounded archive source receipts to already compared inspector reports.

This matrix summarizes exact source evidence. It establishes neither actor-root
membership nor runtime behavior and does not resolve external clip names.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path

from process_guard import guard

def digest(path):
    hasher = hashlib.sha256()
    with path.open('rb') as source:
        while block := source.read(1024 * 1024): hasher.update(block)
    return hasher.hexdigest()

def load(path, maximum=64 * 1024 * 1024):
    if path.stat().st_size > maximum: raise ValueError('matrix input report exceeds its byte admission bound')
    return json.loads(path.read_text(encoding='utf-8'))

def catalogue(report):
    if report.get('runtime_ready') is not False or report.get('failures') != 0:
        raise ValueError('source report has failure or readiness claim')
    if not isinstance(report.get('files'), list) or len(report['files']) > 128:
        raise ValueError('matrix report row count exceeds sample bound')
    result = {}
    for row in report['files']:
        name = Path(row['input']).name
        if name in result: raise ValueError('duplicate report file identity')
        if row.get('error') is not None or row.get('comparison') != 'all_equal':
            raise ValueError('matrix requires a successful exact comparison receipt')
        result[name] = row
    return result

def build(manifest, inputs, animation, skin):
    if manifest.get('runtime_ready') is not False or not isinstance(manifest.get('samples'), list) or len(manifest['samples']) > 128:
        raise ValueError('matrix sample scope is contradictory or exceeds bound')
    animation_rows, skin_rows = catalogue(animation), catalogue(skin)
    if animation.get('schema_version') != 4 or skin.get('schema_version') != 3:
        raise ValueError('matrix requires animation schema4 and skin schema3')
    rows, seen, used_skin = [], set(), set()
    categories = defaultdict(Counter)
    unresolved = Counter()
    for sample in manifest['samples']:
        guard()
        name = sample['file']
        if Path(name).name != name or name in seen: raise ValueError('duplicate or non-leaf sample filename')
        seen.add(name)
        path = inputs / name
        if path.stat().st_size != sample['decoded_bytes'] or digest(path) != sample['sha256']:
            raise ValueError('preserved input bytes/hash differ from source receipt')
        actual = animation_rows.get(name)
        if actual is None or actual['sha256'] != sample['sha256'] or actual['decoded_bytes'] != sample['decoded_bytes'] or actual['tuple'] != sample['tuple'] or actual['container_block_counts'] != sample['block_counts']:
            raise ValueError('animation source identity/tuple/container counts differ')
        counts = Counter()
        for section, data in [('animation', actual['animation']), ('keys', actual['keys']), ('splines', actual['splines']), ('spline_components', actual['spline_components'])]:
            if data['runtime_ready'] is not False or len(data['blocks']) > 100000:
                raise ValueError('catalogue has readiness claim or block count overflow')
            for block in data['blocks']: counts[block['block_type']] += 1
        if any(count > sample['block_counts'].get(kind, 0) for kind, count in counts.items()):
            raise ValueError('decoded source catalogue exceeds original container counts')
        links = Counter(d['target_type'] for d in actual['animation']['dependencies'] if d['kind'] == 'link')
        unresolved.update(links)
        external = sum(d['kind'] == 'external_binding' for d in actual['animation']['dependencies'])
        source_skin = None
        if sample['category'].startswith('nif_'):
            selected = skin_rows.get(name)
            if selected is None or selected['sha256'] != sample['sha256'] or selected['decoded_bytes'] != sample['decoded_bytes'] or selected['tuple'] != sample['tuple']:
                raise ValueError('skin source identity/tuple differs')
            if any(selected[section].get('runtime_ready') is not False for section in ('skin', 'partitions', 'bindings')):
                raise ValueError('skin source catalogue has readiness claim')
            used_skin.add(name)
            source_skin = dict(blocks=len(selected['skin']['blocks']), owners=len(selected['skin']['owners']), partitions=len(selected['partitions']['blocks']), nodes=len(selected['bindings']['nodes']), instances=len(selected['bindings']['instances']), diagnostics=len(selected['bindings']['diagnostics']), unsupported_scene_edges=len(selected['bindings']['unsupported_scene_edges']), ancestry_scope=selected['bindings']['ancestry_scope'], runtime_ready=False)
            categories[sample['category']]['skin_blocks'] += source_skin['blocks']
            categories[sample['category']]['skin_owners'] += source_skin['owners']
            categories[sample['category']]['binding_nodes'] += source_skin['nodes']
        category = categories[sample['category']]
        category['files'] += 1
        category['source_animation_blocks'] += sum(counts.values())
        category['unparsed_animation_links'] += sum(links.values())
        category['external_bindings'] += external
        remainder = {kind:count - counts[kind] for kind,count in sample['block_counts'].items() if count > counts[kind]}
        rows.append(dict(archive=sample['archive'], entry_index=sample['entry_index'], path_bytes=sample['path_bytes'], file=name, category=sample['category'], decoded_bytes=sample['decoded_bytes'], sha256=sample['sha256'], tuple=sample['tuple'], animation_source_counts=dict(counts), unparsed_animation_link_counts=dict(links), unresolved_external_bindings=external, outside_animation_source_catalogue=remainder, skin_source=source_skin, actor_root_membership='not_yet_joined_to_tested_actor_manifest', evaluated_pose=False, retail_behavior_verified=False))
    if seen != set(animation_rows) or used_skin != set(skin_rows): raise ValueError('matrix report/sample row sets differ')
    return dict(schema_version=1, scope='preserved deterministic archive-path/category sample, with exact source comparison receipts; no actor-root join', categories={k:dict(v) for k,v in sorted(categories.items())}, unparsed_animation_link_counts=dict(unresolved), files=rows, runtime_ready=False, evaluated_poses=False, retail_behavior_verified=False)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('manifest', 'inputs', 'animation_report', 'animation_oracle', 'skin_report', 'skin_oracle', 'output'):
        parser.add_argument('--' + name.replace('_', '-'), type=Path, required=True)
    args = parser.parse_args()
    guard()
    # Pretty source keys are larger than the dense native report. Admit one
    # bounded animation tree; the matrix never copies its key/control arrays.
    animation, skin = load(args.animation_report, 128 * 1024 * 1024), load(args.skin_report)
    for report, oracle in [(animation,args.animation_oracle), (skin,args.skin_oracle)]:
        if report.get('oracle_report_sha256') != digest(oracle): raise ValueError('comparison receipt is not bound to the supplied native report')
    result = build(load(args.manifest), args.inputs, animation, skin)
    result['evidence_sha256'] = {name:digest(getattr(args,name)) for name in ('manifest','animation_report','animation_oracle','skin_report','skin_oracle')}
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(result, output, indent=2)
        output.write('\n')
    print(json.dumps(dict(files=len(result['files']), categories=result['categories'], unparsed_animation_link_counts=result['unparsed_animation_link_counts'])))

if __name__ == '__main__': main()
