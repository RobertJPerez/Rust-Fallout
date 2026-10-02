"""Compare selected source fields from full diagnostic and strict deferred indexes."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--full', type=Path, required=True)
parser.add_argument('--deferred', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
full = json.loads(args.full.read_text(encoding='utf-8-sig'))
deferred = json.loads(args.deferred.read_text(encoding='utf-8-sig'))
# Integrity counts describe different scopes. Model cache/probe evidence is also
# separate from the winning records, links and archive candidates compared here.
fields = ['schema_version', 'key', 'editor_id', 'source_plugin', 'record_offset',
          'cell', 'link_failures', 'references', 'models', 'other_child_records']
rows = [{'field': key, 'equal': full[key] == deferred[key]} for key in fields]
strict = deferred['integrity_failures'] == 0 and deferred['link_failures'] == 0 and deferred['index_payloads_deferred'] > 0
summary = {'schema_version': 1, 'fields': rows, 'all_equal': strict and all(r['equal'] for r in rows),
           'full_report_sha256': hashlib.sha256(args.full.read_bytes()).hexdigest(),
           'deferred_report_sha256': hashlib.sha256(args.deferred.read_bytes()).hexdigest(),
           'strict_selected_records': strict, 'index_payloads_deferred': deferred['index_payloads_deferred'],
           'references': len(deferred['references']), 'base_records': len(deferred['models']),
           'scope': 'Selected cell source fields only; full diagnostic input remains tainted; unread bodies are unvalidated'}
with args.output.open('x', encoding='utf-8') as output:
    json.dump(summary, output, indent=2)
    output.write('\n')
print(f"{len(rows)} selected fields/groups; all equal: {summary['all_equal']}")
raise SystemExit(0 if summary['all_equal'] else 1)
