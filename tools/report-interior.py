"""Publish reviewed cell capture metadata while keeping images and source assets local."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--captures', type=Path, required=True)
parser.add_argument('--selection-comparison', type=Path, required=True)
parser.add_argument('--house-comparison', type=Path, required=True)
parser.add_argument('--variant-comparison', type=Path, required=True)
parser.add_argument('--negative-checks', type=Path, required=True)
parser.add_argument('--visually-reviewed', action='store_true', required=True)
args = parser.parse_args()


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


manifest = read(args.captures / 'manifest.json')
selection = read(args.selection_comparison)
comparisons = [read(args.house_comparison), read(args.variant_comparison)]
negative = read(args.negative_checks)
if not manifest['all_captured'] or digest(ROOT / manifest['binary']) != manifest['binary_sha256']:
    raise ValueError('GPU capture failed or executable changed')
if not selection['all_equal'] or not all(c['all_equal'] for c in comparisons) or not negative['all_rejected']:
    raise ValueError('Selected fields, independent model comparison or negative checks failed')
captures, documents = [], []
for row in manifest['results']:
    png, report = args.captures / row['png'], args.captures / row['report']
    if digest(png) != row['png_sha256'] or digest(report) != row['report_sha256']:
        raise ValueError('Capture artifacts changed')
    document = read(report)
    if document['cell']['integrity_failures'] or document['cell']['link_failures'] or document['runtime_ready']:
        raise ValueError('Expected strict selected records with explicit incomplete runtime status')
    captures.append(row)
    documents.append(document)
if documents[0] != documents[1]:
    raise ValueError('Cell preparation differed between camera fixtures')
cell = documents[0]
gaps = ['No measured retail placement/camera or image comparison; rotation convention is provisional',
        'Unlit diffuse materials; magenta fallback surfaces are visible in the bedroom window',
        'Standalone audio/heading marker models remain visible; only declared BSX marker submeshes are omitted',
        'Actor models, alternate item model selection, enable parents, controllers and skinning are incomplete',
        'No collision, player simulation, quests, scripts, saving or accepted gameplay scenarios',
        'Fly/orbit keyboard bindings are implemented but not independently exercised by keyboard automation',
        'Startup work is synchronous; disk indexes, streaming and startup performance remain open',
        'Whole-corpus integrity defects and archive/loose precedence remain unresolved']
summary = {'schema_version': 1, 'cell_editor_id': manifest['cell_editor_id'], 'cell_key': cell['cell']['key'],
           'binary_sha256': manifest['binary_sha256'], 'capture_manifest_sha256': digest(args.captures/'manifest.json'),
           'captures_directory': str(args.captures), 'captures': captures, 'all_exit_codes_zero': True,
           'visual_review': 'Both views inspected: recognizable textured room, furniture, cabinet and bed; diagnostic markers and unsupported magenta surfaces remain visible',
           'source_origin': cell['source_origin'], 'coordinates': cell['coordinates'],
           'load_order': cell['load_order'], 'load_order_sha256': cell['load_order_sha256'], 'plugin_sha256': cell['plugin_sha256'],
           'references_inspected': len(cell['placements']), 'placement_outcomes': dict(Counter(p['status'] for p in cell['placements'])),
           **{key:cell[key] for key in ['unique_render_models','rendered_references','rendered_mesh_instances',
                                      'shared_geometry_vertices','shared_geometry_triangles','unique_texture_samplers']},
           'selected_records_integrity_failures': cell['cell']['integrity_failures'],
           'selected_records_link_failures': cell['cell']['link_failures'],
           'index_payloads_deferred': cell['cell']['index_payloads_deferred'], 'selection_comparison': selection,
           'independent_model_comparison': {'files': sum(c['files'] for c in comparisons), 'all_equal': True,
               'extra_flag_blocks': sum(r['extra_flags'] for c in comparisons for r in c['results']),
               'report_sha256': [digest(args.house_comparison), digest(args.variant_comparison)]},
           'negative_comparison_checks': negative, 'model_failures': sum(m['error'] is not None for m in cell['models']),
           'declared_editor_marker_meshes_omitted': sum('Omitted declared editor marker' in w for m in cell['models'] if m['report'] for w in m['report']['warnings']),
           'known_gaps': gaps, 'runtime_ready': False, 'retail_parity_accepted': False}
(ROOT / 'reports/interior-preview.json').write_text(json.dumps(summary, indent=2)+'\n', encoding='utf-8')
ledger_path = ROOT / 'parity/requirements.json'
ledger = read(ledger_path)
identifier = 'fnv.presentation.interior_preview'
ledger['requirements'] = [r for r in ledger['requirements'] if r['id'] != identifier]
ledger['requirements'].append({'id': identifier, 'game_profile': digest(ROOT/'profiles/manifest.json'),
    'subsystem': 'presentation', 'requirement': 'Assemble a real interior from strict winning references with shared immutable models and textures',
    'status': 'unit-tested', 'affected_content': 'GSDocMitchellHouse; 435 inspected / 400 rendered references',
    'evidence': ['reports/interior-preview.json','docs/interior-preview.md'],
    'tests': ['crates/fallout-data/src/coordinates.rs','tools/smoke-interior.py','tools/compare-cell-selection.py'],
    'known_gaps': gaps, 'tolerance': 'Exact selected source fields; successful GPU captures and visual inspection; no retail image tolerance',
    'last_verified_revision': None})
ledger_path.write_text(json.dumps(ledger, indent=2)+'\n', encoding='utf-8')
print(f"Published {len(captures)} reviewed interior views; {summary['rendered_references']} rendered references")
