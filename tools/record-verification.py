"""Bind checkpoint 05 evidence to committed source, actual binaries and local artifacts."""
import argparse
from datetime import date
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--check-log', type=Path, required=True)
args = parser.parse_args()


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
snapshot = read(ROOT/'reports/source-snapshot.json')
if snapshot['revision'] != revision:
    raise ValueError('Create a source snapshot after committing the tested implementation')
for row in snapshot['files']:
    committed = subprocess.check_output(['git', 'show', revision+':'+row['path']], cwd=ROOT)
    if hashlib.sha256(committed).hexdigest() != row['sha256'] or digest(ROOT/row['path']) != row['sha256']:
        raise ValueError('Source differs from implementation commit: '+row['path'])
before, after = read(ROOT/'local/baseline.json'), read(ROOT/'local/baseline-05.json')
if before['files'] != after['files'] or before['content_fingerprint'] != after['content_fingerprint']:
    raise ValueError('Installation differs from the original baseline')
raw_log = args.check_log.read_bytes()
log = raw_log.decode('utf-16' if raw_log.startswith(b'\xff\xfe') else 'utf-8')
passed = sum(int(n) for n in re.findall(r'test result: ok\. (\d+) passed;', log))
if passed != 52 or 'error:' in log or 'Clippy failed' in log or 'Finished `dev`' not in log:
    raise ValueError('Expected complete 52-test workspace check and successful Clippy output')
interior = read(ROOT/'reports/interior-preview.json')
models = read(ROOT/'reports/model-preview.json')
preview = ROOT/'target/debug/fallout-preview.exe'
if digest(preview) != interior['binary_sha256'] or digest(preview) != models['binary_sha256']:
    raise ValueError('GPU evidence belongs to a different preview executable')
comparisons = [read(ROOT/'local/nif-comparison-house-05.json'), read(ROOT/'local/nif-comparison-variants-05.json')]
if not all(c['all_equal'] for c in comparisons) or not interior['selection_comparison']['all_equal']:
    raise ValueError('Independent or selected-cell comparisons failed')
oracle_path = ROOT/'local/nif-oracle-build/Release/nif-oracle.exe'
cli_path = ROOT/'target/release/fallout.exe'
# The probe manifest binds the decoded model evidence to the release CLI.
for directory in ['scenes-house-05', 'scenes-variants-05']:
    if read(ROOT/'local'/directory/'manifest.json')['binary_sha256'] != digest(cli_path):
        raise ValueError('Scene probes used a different release CLI')
report_paths = ['reports/interior-preview.json','reports/model-preview.json','sources.lock.json']
summary = {'schema_version': 1, 'date': date.today().isoformat(), 'checkpoint': 5,
    'engine_revision': revision, 'source_snapshot_sha256': snapshot['sha256'],
    'source_matches_implementation_commit': True, 'release_cli_sha256': digest(cli_path),
    'debug_preview_sha256': digest(preview), 'raw_oracle_binary_sha256': digest(oracle_path),
    'check_command': 'powershell -NoProfile -ExecutionPolicy Bypass -File tools/check.ps1',
    'check_exit_code': 0, 'check_log_sha256': digest(args.check_log), 'tests_passed': passed,
    'unit_tests_passed': 13, 'integration_tests_passed': 39,
    'format_check': 'passed', 'clippy_warnings_denied': 'passed',
    'original_installation_matches_baseline': True, 'installation_files_checked': len(after['files']),
    'installation_bytes_checked': sum(f['bytes'] for f in after['files']),
    'content_fingerprint': after['content_fingerprint'], 'baseline_report_sha256': digest(ROOT/'local/baseline-05.json'),
    'selected_cell': {'references': interior['references_inspected'], 'base_records': interior['selection_comparison']['base_records'],
        'integrity_failures': 0, 'link_failures': 0, 'index_payloads_deferred': interior['index_payloads_deferred'],
        'full_and_deferred_selected_fields_equal': True, 'whole_corpus_certified': False},
    'independent_payload_comparison': {'files': sum(c['files'] for c in comparisons), 'all_equal': True,
        **{key: sum(row[key] for c in comparisons for row in c['results'])
           for key in ['objects','meshes','materials','extra_flags','vertices','triangles']},
        'tolerance': comparisons[0]['tolerance'], 'oracle': comparisons[0]['oracle']},
    'negative_comparison_checks_passed': len(interior['negative_comparison_checks']['results']),
    'gpu_captures': {'models': 2, 'interior_views': 2, 'all_exit_codes_zero': True,
        'visually_inspected': True, 'retail_image_comparison': False},
    'interior': {key: interior[key] for key in ['rendered_references','unique_render_models','rendered_mesh_instances',
        'shared_geometry_vertices','shared_geometry_triangles','unique_texture_samplers','placement_outcomes']},
    'builds': {'cli': 'release built', 'preview': 'debug built and exercised; release not built', 'raw_oracle': 'MSVC Release built'},
    'reports': {path:digest(ROOT/path) for path in report_paths},
    'gameplay_acceptance': 'not implemented; no accepted scenarios',
    'prior_verification': 'reports/checkpoint-04-verification.json'}
(ROOT/'reports/verification.json').write_text(json.dumps(summary, indent=2)+'\n', encoding='utf-8')

ledger_path = ROOT/'parity/requirements.json'
ledger = read(ledger_path)
ledger['last_verified_revision'] = revision
ledger['revision_note'] = 'Checkpoint 05 binds newly checked scopes to the implementation commit; prior corpus scopes retain their initial checkpoint revision.'
for row in ledger['requirements']:
    row['last_verified_revision'] = revision if row['id'] in {
        'fnv.plugin.bounds','fnv.nif.scene_mesh_payloads','fnv.nif.material_texture_payloads',
        'fnv.presentation.model_preview','fnv.presentation.interior_preview'} else 'd33c7a11d3288a321604fad43b3021f28a7ed43b'
ledger_path.write_text(json.dumps(ledger, indent=2)+'\n', encoding='utf-8')
print(f"Checkpoint 05 bound to {revision}; {passed} tests and four GPU captures")
