"""Record resolved Cargo license declarations without claiming a per-file audit."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('metadata', type=Path)
args = parser.parse_args()
metadata = json.loads(args.metadata.read_text(encoding='utf-8-sig'))
result = {
    'scope': 'Workspace Cargo metadata, including Bevy preview and offline archive oracle; separate GPL plugin and CMake oracles excluded.',
    'limitations': 'Manifest declarations, not a completed per-file/transitive source audit.',
    'packages': [{key: package.get(key) for key in ['name','version','source','license','license_file','repository']}
                 for package in sorted(metadata['packages'], key=lambda p: (p['name'],p['version']))],
}
output = Path(__file__).resolve().parents[1] / 'reports/dependency-licenses.json'
output.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
print(f"Recorded {len(result['packages'])} resolved package declarations")
