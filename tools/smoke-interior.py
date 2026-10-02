"""Capture two source-coordinate views of the real interior, retaining every omission."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--install', type=Path, required=True)
parser.add_argument('--load-order', type=Path, required=True)
parser.add_argument('--output-dir', type=Path, required=True)
parser.add_argument('--binary', type=Path, default=Path('target/debug/fallout-preview.exe'))
args = parser.parse_args()
destination, installation = args.output_dir.resolve(), args.install.resolve()
if destination == installation or installation in destination.parents:
    raise ValueError('Capture output must be outside the installation')
destination.mkdir(parents=True, exist_ok=False)


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


binary_digest, order_digest = digest(args.binary), digest(args.load_order)
results = []
for name, position, target in [
    ('vigortester', [2130, 2130, 7440], [1883, 1763, 7420]),
    ('bedroom', [2030, 1250, 7450], [2312, 951, 7420]),
]:
    png, report = destination / (name+'.png'), destination / (name+'.json')
    command = [str(args.binary.resolve()), '--install', str(args.install), '--cell', 'GSDocMitchellHouse',
               '--load-order', str(args.load_order), '--camera-position', *map(str, position),
               '--camera-look-at', *map(str, target), '--headless', '--capture', str(png), '--report', str(report)]
    started = time.monotonic()
    run = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=240)
    (destination / (name+'.log')).write_bytes(run.stdout)
    row = {'view': name, 'source_position': position, 'source_target': target,
           'exit_code': run.returncode, 'seconds': time.monotonic()-started}
    if run.returncode == 0 and png.exists() and report.exists():
        row.update(png=png.name, png_sha256=digest(png), report=report.name, report_sha256=digest(report))
    results.append(row)
if digest(args.binary) != binary_digest or digest(args.load_order) != order_digest:
    raise ValueError('Executable or load order changed during capture')
summary = {'schema_version': 1, 'cell_editor_id': 'GSDocMitchellHouse',
           'binary': str(args.binary), 'binary_sha256': binary_digest,
           'load_order_sha256': order_digest, 'results': results,
           'all_captured': all(row['exit_code'] == 0 and 'png_sha256' in row for row in results),
           'visual_inspection': 'separate manual check required', 'retail_parity_accepted': False}
(destination / 'manifest.json').write_text(json.dumps(summary, indent=2)+'\n', encoding='utf-8')
print(f"{len(results)} interior GPU captures; all completed: {summary['all_captured']}")
raise SystemExit(0 if summary['all_captured'] else 1)
