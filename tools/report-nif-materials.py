"""Publish checkpoint 04 evidence without treating successful decoding as game parity."""
from collections import Counter
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL = ROOT / 'local'
MATERIALS = {'NiMaterialProperty', 'NiAlphaProperty', 'NiStencilProperty', 'NiShadeProperty',
             'BSShaderPPLightingProperty', 'BSShaderNoLightingProperty', 'BSShaderTextureSet',
             'NiSourceTexture', 'NiTexturingProperty'}


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def main():
    census_path = LOCAL / 'nif-material-census-final.json'
    census = read(census_path)
    comparisons = [read(LOCAL / name) for name in ['nif-material-comparison-final.json', 'nif-material-variants-comparison-final.json']]
    negatives = read(LOCAL / 'nif-material-negative-checks-final.json')
    assets = read(LOCAL / 'nif-assets-docmitchell-final.json')
    baseline = read(LOCAL / 'baseline.json')
    archive_hashes = {Path(row['path']).name: row['sha256'] for row in baseline['files'] if row['path'].lower().endswith('.bsa')}
    unchanged = all(archive_hashes.get(Path(a['archive']).name) == a['archive_sha256'] for a in census['archives'])
    if not unchanged or not all(c['all_equal'] for c in comparisons) or not negatives['all_rejected'] or assets['failures']:
        raise ValueError('Publication requires unchanged archives, matching independent comparisons and complete house texture resolution')
    for row in assets['textures']:
        if not row['cache'] or len(row['candidates']) != 1:
            raise ValueError('Expected a unique, cached house texture')
        if digest(LOCAL / 'docmitchell-textures' / (row['cache']['key'] + '.blob')) != row['sha256']:
            raise ValueError('Texture cache digest changed')
    counts, decoded, unsupported, compared = Counter(), Counter(), Counter(), Counter()
    problems = []
    for archive in census['archives']:
        scene = archive['scene_payloads']
        counts.update({key: value for key, value in scene.items() if isinstance(value, int)})
        decoded.update(scene['decoded_blocks'])
        unsupported.update(scene['unsupported_blocks'])
        problems.extend({'archive': Path(archive['archive']).name, **row} for row in scene['texture_path_failures'])
    for directory in ['materials-docmitchell-final', 'materials-variants-final']:
        path = LOCAL / directory
        for row in read(path / 'manifest.json')['results']:
            report = read(path / row['report'])
            index = report['index']
            for material in report['scene']['materials']:
                compared[index['block_types'][index['blocks'][material['block']]['type_index']]] += 1
    profile_hash = digest(ROOT / 'profiles/manifest.json')
    summary = {
        'schema_version': 1, 'profile': 'nv-original', 'profile_manifest_sha256': profile_hash,
        'archive_hashes_match_original_baseline': unchanged, 'archives': len(census['archives']),
        'successful_scene_files': dict(counts), 'legacy_container_failures': census['failures'], 'scene_failures': census['scene_failures'],
        'independent_comparison': {
            'files': sum(c['files'] for c in comparisons), 'all_equal': True,
            **{key: sum(row[key] for c in comparisons for row in c['results']) for key in ['objects','meshes','materials','vertices','triangles']},
            'tolerance': comparisons[0]['tolerance'], 'oracle': comparisons[0]['oracle'],
        },
        'house_texture_dependencies': {
            'models': len(assets['models']), 'material_blocks': sum(m['material_blocks'] for m in assets['models']),
            'references': sum(len(m['textures']) for m in assets['models']), 'unique_paths': len(assets['textures']),
            'decoded_bytes': sum(t['decoded_bytes'] for t in assets['textures']), 'failures': assets['failures'],
            'all_cached_payload_hashes_verified': True, 'retail_shader_semantics_verified': False,
        },
        'negative_checks': negatives, 'texture_path_failures': problems,
        'texture_failure_samples_complete': len(problems) == counts['unsafe_texture_paths'],
        'decoded_material_blocks': {key: decoded[key] for key in sorted(MATERIALS)},
        'oracle_compared_material_blocks': dict(compared),
        'known_gaps': ['Retail shader evaluation, normal/specular/environment maps and property inheritance',
            'Controller evaluation, skinning, collision and unsupported scene branches',
            'Seven unsafe authored paths remain unresolved; no exporter-path repair is assumed',
            'Archive/loose precedence and complete dependency closure',
            'Six legacy containers and 25 prior strict geometry rejections remain open'],
        'raw_reports': {str(p.relative_to(ROOT)).replace('\\','/'): digest(p) for p in [census_path,
            LOCAL / 'nif-material-comparison-final.json', LOCAL / 'nif-material-variants-comparison-final.json',
            LOCAL / 'nif-material-negative-checks-final.json', LOCAL / 'nif-assets-docmitchell-final.json']},
        'runtime_ready': False, 'accepted_scenarios': [],
    }
    write(ROOT / 'reports/nif-materials.json', summary)
    write(ROOT / 'parity/nif-material-coverage.json', {
        'schema_version': 1, 'profile_manifest_sha256': profile_hash,
        'scope': 'Material payload decoding; occurrence counts only in successful scene files; independent comparison on samples',
        'block_types': [{'name': name, 'decoded_occurrences': decoded[name], 'oracle_compared_occurrences': compared[name],
                         'status': 'oracle-tested' if compared[name] else 'synthetic-tested; no compared retail sample'} for name in sorted(MATERIALS)],
        'rendering_accepted': False,
    })
    coverage_path = ROOT / 'parity/nif-coverage.json'
    coverage = read(coverage_path)
    for row in coverage['block_types']:
        if row['name'] in MATERIALS:
            row['payload_semantics'] = 'material fields decoded; see nif-material-coverage.json for independent sample coverage'
    write(coverage_path, coverage)
    ledger_path = ROOT / 'parity/requirements.json'
    ledger = read(ledger_path)
    for row in ledger['requirements']:
        if row['id'] == 'fnv.nif.scene_mesh_payloads':
            row['known_gaps'] = ['Unknown scene branches and the 25 strict payload failures remain open',
                'Controller evaluation, skinning and collision are absent',
                'Material decoding and model-only presentation are tracked separately; no retail behavior acceptance']
    identifier = 'fnv.nif.material_texture_payloads'
    ledger['requirements'] = [row for row in ledger['requirements'] if row['id'] != identifier]
    ledger['requirements'].append({
        'id': identifier, 'game_profile': profile_hash, 'subsystem': 'formats',
        'requirement': 'Decode material fields and follow authored external texture paths through unique immutable archive members',
        'status': 'oracle-tested', 'affected_content': 'NV archived models; exact material comparisons on 218 sample files',
        'evidence': ['reports/nif-materials.json', 'parity/nif-material-coverage.json', 'docs/nif-materials.md'],
        'tests': ['crates/fallout-data/tests/nif_scene.rs', 'tools/compare-nif-scenes.py', 'tools/check-nif-scene-comparison.py'],
        'known_gaps': summary['known_gaps'], 'tolerance': comparisons[0]['tolerance'], 'last_verified_revision': None,
    })
    write(ledger_path, ledger)
    print(f"Published {counts['materials']} material blocks; {len(assets['textures'])} verified house textures")


if __name__ == '__main__':
    main()
