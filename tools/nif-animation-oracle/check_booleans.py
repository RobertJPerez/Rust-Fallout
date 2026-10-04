"""Exact Boolean source projections, raw source identity and intended refusals."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

from boolean_fixtures import VARIANTS, expected, payloads, source
from fixtures import STREAMS, w
from process_guard import guard, run

def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def write(path, document):
    with path.open('x', encoding='utf-8') as output:
        json.dump(document, output, indent=2); output.write('\n')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--oracle', type=Path, required=True)
    parser.add_argument('--retail-inputs', type=Path, required=True)
    parser.add_argument('--frozen-schema4-oracle', type=Path, required=True)
    parser.add_argument('--layout-audit', type=Path, required=True)
    args = parser.parse_args()
    guard()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    inputs = args.output_dir / 'inputs'; inputs.mkdir()
    for stream in STREAMS:
        for variant in VARIANTS:
            (inputs / f'stream-{stream}-{variant}.blob').write_bytes(source(stream, variant))
    original_hashes = {p.name:dict(bytes=p.stat().st_size, sha256=digest(p)) for p in args.retail_inputs.iterdir()}
    authored_hashes = {p.name:digest(p) for p in inputs.iterdir()}
    binaries = [digest(args.binary), digest(args.oracle)]
    write(args.output_dir / 'original-inputs-before.json', original_hashes)
    flags = {4:['--include-spline-components'], 5:['--include-bool-interpolators']}
    def native(name, directory, schema=5):
        path = args.output_dir / f'{name}-oracle.json'
        with path.open('xb') as output:
            result = run([str(args.oracle.resolve()), str(directory.resolve()), *flags[schema]], stdout=output, stderr=subprocess.PIPE)
        (args.output_dir / f'{name}-native.stderr.txt').write_bytes(result.stderr)
        document = json.loads(path.read_text())
        assert document['oracle_binary_sha256'] == binaries[1]
        return result.returncode, path, document
    def rust(name, directory, oracle=None, schema=5):
        path = args.output_dir / f'{name}-rust.json'
        command = [str(args.binary.resolve()), 'nif-animation', str(directory.resolve()), '--output', str(path.resolve()), *flags[schema]]
        if oracle: command += ['--oracle-report', str(oracle.resolve())]
        result = run(command, capture_output=True)
        (args.output_dir / f'{name}-rust.stdout.txt').write_bytes(result.stdout)
        (args.output_dir / f'{name}-rust.stderr.txt').write_bytes(result.stderr)
        document = json.loads(path.read_text()) if path.exists() else None
        return result.returncode, document, result.stderr.decode('utf-8', errors='replace')
    native_code, native_path, oracle = native('authored', inputs)
    code, document, _ = rust('authored', inputs, native_path)
    assert not native_code and not code, 'authored comparison failed; evidence retained'
    assert len(document['files']) == 96 and document['failures'] == 0 and not document['runtime_ready']
    for row in oracle['files']:
        variant = row['file'].split('-',2)[2][:-5]
        assert [b['data'] for b in row['bool_interpolators']] == expected(variant)
        assert [b['block_type'] for b in row['bool_interpolators']] == ['NiBoolInterpolator','NiBoolTimelineInterpolator']
        assert all(b['bytes'] == 5 for b in row['bool_interpolators'])
    def row(d): return next(r for r in d['files'] if r['file'] == 'stream-34-baseline.blob')
    def block(d): return row(d)['bool_interpolators'][0]
    changes = {
        'block':lambda d:block(d).update(block=0),
        'type':lambda d:block(d).update(block_type='NiBoolTimelineInterpolator'),
        'span':lambda d:block(d).update(bytes=6),
        'offset':lambda d:block(d).update(offset=0),
        'hash':lambda d:block(d).update(sha256='0'*64),
        'truth_conversion':lambda d:block(d)['data'].update(raw_value=1),
        'null_conversion':lambda d:block(d)['data'].update(data=None),
        'extra_semantics':lambda d:block(d)['data'].update(truth=True),
        'order':lambda d:row(d)['bool_interpolators'].reverse(),
        'missing_block':lambda d:row(d)['bool_interpolators'].pop(),
        'branch':lambda d:d.update(bool_interpolator_branch='other'),
        'raw_contract':lambda d:d.update(raw_bool_fields_checked=False),
        'schema':lambda d:d.update(schema_version=4),
    }
    negatives = []
    for name, mutate in changes.items():
        guard(); changed = copy.deepcopy(oracle); mutate(changed)
        path = args.output_dir / f'altered-{name}.json'; write(path, changed)
        code, report, stderr = rust(name, inputs, path)
        reason = 'source/provenance contract' if name in ('branch','raw_contract','schema') else 'Boolean interpolator identity/span/hash or source fields differ'
        assert code and reason in stderr + json.dumps(report), name
        negatives.append(dict(name=name, exit_code=code, intended_reason=reason))
    malformed = []
    malformed_inputs = args.output_dir / 'malformed-inputs'
    malformed_inputs.mkdir()
    ordinary, timeline = payloads('baseline')
    for field, payload in [('ordinary',ordinary),('timeline',timeline)]:
        for name, raw, rust_reason, native_reason in [
            ('short',payload[:-1],'field exceeds','source span differs'),
            ('surplus',payload+b'\x00','unconsumed bytes','source span differs'),
            ('range',payload[:1]+w(15),'block index out of range','source link out of range'),
            ('wrong_kind',payload[:1]+w(12),'Boolean data link has wrong target kind','Boolean data link has wrong target kind'),
            ('self_kind',payload[:1]+w(8),'Boolean data link has wrong target kind','Boolean data link has wrong target kind'),
        ]:
            name = field + '-' + name
            path = malformed_inputs / f'malformed-{name}.blob'; path.write_bytes(source(34,'baseline',{field:raw}))
            native_code, _, native_report = native('malformed-'+name,path)
            code, report, stderr = rust('malformed-'+name,path)
            assert native_code and native_reason in json.dumps(native_report),name
            assert code and rust_reason in stderr + json.dumps(report),name
            old_native, old_path, _ = native('opaque-'+name,path,4)
            old_code, old_report, _ = rust('opaque-'+name,path,old_path,4)
            assert not old_native and not old_code and old_report['failures']==0,name
            malformed.append(dict(name=name,native_reason=native_reason,rust_reason=rust_reason))
    native_code, retail_path, retail = native('retail',args.retail_inputs)
    code, actual, _ = rust('retail',args.retail_inputs,retail_path)
    assert not native_code and not code and actual['failures']==0
    old = json.loads(args.frozen_schema4_oracle.read_text())
    assert len(retail['files']) == len(old['files']) == 70
    for fresh, frozen in zip(retail['files'],old['files']):
        assert fresh['file']==frozen['file']
        projected={key:value for key,value in fresh.items() if key!='bool_interpolators'}
        assert projected==frozen,'earlier source projection changed'
    counts={kind:sum(b['block_type']==kind for row in retail['files'] for b in row['bool_interpolators']) for kind in ('NiBoolInterpolator','NiBoolTimelineInterpolator')}
    assert counts=={'NiBoolInterpolator':252,'NiBoolTimelineInterpolator':3}
    audit = json.loads(args.layout_audit.read_text())
    for row in retail['files']:
        raw = [b for b in audit['blocks'] if b['file'] == row['file']]
        assert len(raw) == len(row['bool_interpolators'])
        for block, independent in zip(row['bool_interpolators'], raw):
            assert all(block[key] == independent[key] for key in ('block','block_type','offset','bytes','sha256'))
            assert block['data'] == dict(raw_value=independent['raw_value'],data=independent['data_link'])
        assert audit['inputs'][row['file']] == dict(bytes=row['decoded_bytes'],sha256=row['sha256'])
    summary=dict(authored_files=96,altered_reports=len(negatives),malformed_inputs=len(malformed),negatives=negatives,malformed=malformed,originals=dict(files=len(retail['files']),counts=counts,work=sum(r['bool_interpolators']['work_units'] for r in actual['files']),retained_bytes=sum(r['bool_interpolators']['retained_bytes'] for r in actual['files']),dependencies=sum(len(r['bool_interpolators']['dependencies']) for r in actual['files']),remaining_dependencies=actual['unresolved_dependencies'],diagnostics=actual['diagnostics']),runtime_ready=False,bool_evaluation=False,timeline_events_verified=False)
    assert original_hashes=={p.name:dict(bytes=p.stat().st_size,sha256=digest(p)) for p in args.retail_inputs.iterdir()}
    assert authored_hashes=={p.name:digest(p) for p in inputs.iterdir()}
    assert binaries==[digest(args.binary),digest(args.oracle)]
    write(args.output_dir / 'summary.json',summary)
    print(json.dumps(summary))

if __name__ == '__main__': main()
