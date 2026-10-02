"""Publish reviewed GPU smoke captures as model-inspection evidence, not retail parity."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--captures', type=Path, required=True)
parser.add_argument('--visually-reviewed', action='store_true', required=True)
args = parser.parse_args()


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2)+'\n', encoding='utf-8')


captures = read(args.captures / 'manifest.json')
if not captures['all_captured'] or digest(ROOT / captures['binary']) != captures['binary_sha256']:
    raise ValueError('Capture did not complete or executable changed')
rows = []
for row in captures['results']:
    report_path, png = args.captures / row['report'], args.captures / row['png']
    if digest(report_path) != row['report_sha256'] or digest(png) != row['png_sha256']:
        raise ValueError('Capture artifact changed')
    report = read(report_path)
    rows.append({**row, **{key: report[key] for key in ['model_sha256','meshes','vertices','triangles','textures','warnings']}})
summary = {
    'schema_version': 1, 'binary': captures['binary'], 'binary_sha256': captures['binary_sha256'],
    'captures_directory': str(args.captures), 'captures_manifest_sha256': digest(args.captures / 'manifest.json'),
    'captures': rows, 'all_exit_codes_zero': True,
    'visual_review': 'Both PNGs inspected: recognizable textured models, fully framed; no blank frame or magenta surface visible in these views',
    'backend': 'Bevy 0.19.1 / wgpu; headless image target and asynchronous GPU readback',
    'rendering': 'unlit diffuse; source UVs and source units; [x,y,z] -> [x,z,-y]',
    'known_gaps': ['No retail image comparison or lighting/shader parity; interior assembly is tracked separately',
        'Skinning, controllers, collision and gameplay are absent',
        'Cabinet report records one unsupported diffuse binding; the chosen view does not prove that surface renders correctly',
        'Interactive orbit bindings implemented but not independently exercised with keyboard automation'],
    'retail_parity_accepted': False,
}
write(ROOT / 'reports/model-preview.json', summary)
ledger_path = ROOT / 'parity/requirements.json'
ledger = read(ledger_path)
identifier = 'fnv.presentation.model_preview'
ledger['requirements'] = [row for row in ledger['requirements'] if row['id'] != identifier]
ledger['requirements'].append({
    'id': identifier, 'game_profile': digest(ROOT / 'profiles/manifest.json'), 'subsystem': 'presentation',
    'requirement': 'Render supported archived NIF geometry and diffuse DDS through a separate Bevy adapter',
    'status': 'unit-tested', 'affected_content': 'Two real model smoke fixtures; not a complete cell',
    'evidence': ['reports/model-preview.json','docs/model-preview.md'],
    'tests': ['crates/fallout-preview/src/model.rs','tools/smoke-preview.py'],
    'known_gaps': summary['known_gaps'], 'tolerance': 'Successful GPU capture and visual smoke inspection only; no retail comparison tolerance defined',
    'last_verified_revision': None,
})
write(ledger_path, ledger)
print(f'Published {len(rows)} reviewed model captures')
