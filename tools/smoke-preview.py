"""Capture two real models and bind the reports/PNGs to the executable used."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--install', type=Path, required=True)
parser.add_argument('--output-dir', type=Path, required=True)
parser.add_argument('--binary', type=Path, default=Path('target/debug/fallout-preview.exe'))
args = parser.parse_args()
args.output_dir.mkdir(parents=True, exist_ok=False)


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


binary_digest = digest(args.binary)
results = []
for name, model in [('chair', 'meshes/furniture/chair01.nif'),
                    ('vitomatic', 'meshes/architecture/goodsprings/nv_vitomaticvigortester_cabinet02.nif')]:
    png, report = args.output_dir / (name+'.png'), args.output_dir / (name+'.json')
    command = [str(args.binary.resolve()), '--install', str(args.install), '--model', model,
               '--headless', '--capture', str(png), '--report', str(report)]
    started = time.monotonic()
    run = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
    (args.output_dir / (name+'.log')).write_bytes(run.stdout)
    row = {'model': model, 'exit_code': run.returncode, 'seconds': time.monotonic()-started}
    if run.returncode == 0 and png.exists() and report.exists():
        row.update(png=png.name, png_sha256=digest(png), report=report.name, report_sha256=digest(report))
    results.append(row)
if digest(args.binary) != binary_digest:
    raise ValueError('Executable changed during capture')
summary = {'schema_version': 1, 'binary': str(args.binary), 'binary_sha256': binary_digest,
           'results': results, 'all_captured': all(row['exit_code'] == 0 and 'png_sha256' in row for row in results),
           'visual_inspection': 'separate manual check required', 'retail_parity_accepted': False}
(args.output_dir / 'manifest.json').write_text(json.dumps(summary, indent=2)+'\n', encoding='utf-8')
print(f"{len(results)} GPU captures; all completed: {summary['all_captured']}")
raise SystemExit(0 if summary['all_captured'] else 1)
