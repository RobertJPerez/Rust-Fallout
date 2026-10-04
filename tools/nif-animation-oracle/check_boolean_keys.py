"""Exact constant Boolean key sources, raw order and intended refusals."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

from boolean_key_fixtures import VARIANTS, expected, payload, source
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
    parser.add_argument('--frozen-schema5-oracle', type=Path, required=True)
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
    flags = {5:['--include-bool-interpolators'], 6:['--include-bool-keys']}
    def native(name, directory, schema=6):
        path = args.output_dir / f'{name}-oracle.json'
        with path.open('xb') as output:
            result = run([str(args.oracle.resolve()), str(directory.resolve()), *flags[schema]], stdout=output, stderr=subprocess.PIPE)
        (args.output_dir / f'{name}-native.stderr.txt').write_bytes(result.stderr)
        document = json.loads(path.read_text())
        assert document['oracle_binary_sha256'] == binaries[1]
        return result.returncode, path, document
    def rust(name, directory, oracle=None, schema=6):
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
        assert [b['data'] for b in row['bool_keys']] == [expected(variant)]
        assert [b['block_type'] for b in row['bool_keys']] == ['NiBoolData']
    def row(d): return next(r for r in d['files'] if r['file'] == 'stream-34-baseline.blob')
    def block(d): return row(d)['bool_keys'][0]
    def group(d): return block(d)['data']
    changes = {
        'block':lambda d:block(d).update(block=0),
        'type':lambda d:block(d).update(block_type='NiBoolInterpolator'),
        'span':lambda d:block(d).update(bytes=6),
        'offset':lambda d:block(d).update(offset=0),
        'hash':lambda d:block(d).update(sha256='0'*64),
        'declared_count':lambda d:group(d).update(declared_keys=1),
        'tag':lambda d:group(d).update(key_type=1),
        'truth_conversion':lambda d:group(d)['keys'][0].update(raw_value=1),
        'time':lambda d:group(d)['keys'][0].update(time_bits=1),
        'extra_semantics':lambda d:group(d).update(truth=True),
        'order':lambda d:group(d)['keys'].reverse(),
        'missing_key':lambda d:group(d)['keys'].pop(),
        'missing_block':lambda d:row(d)['bool_keys'].pop(),
        'branch':lambda d:d.update(bool_key_branch='other'),
        'raw_contract':lambda d:d.update(raw_bool_key_counts_checked=False),
        'schema':lambda d:d.update(schema_version=5),
    }
    negatives = []
    for name, mutate in changes.items():
        guard(); changed = copy.deepcopy(oracle); mutate(changed)
        path = args.output_dir / f'altered-{name}.json'; write(path, changed)
        code, report, stderr = rust(name, inputs, path)
        reason = 'source/provenance contract' if name in ('branch','raw_contract','schema') else 'Boolean-key identity/span/hash or ordered source fields differ'
        assert code and reason in stderr + json.dumps(report), name
        negatives.append(dict(name=name, exit_code=code, intended_reason=reason))
    malformed = []
    malformed_inputs = args.output_dir / 'malformed-inputs'
    malformed_inputs.mkdir()
    baseline = payload('baseline')
    cases = [
        ('short_count',b'\x00','field exceeds','source span'),
        ('missing_tag',w(1),'field exceeds','source span'),
        ('short_key',baseline[:-1],'array exceeds','count/span budget'),
        ('surplus_key',baseline+b'\x00','unconsumed bytes','surplus bytes'),
        ('zero_surplus',w(0,5),'unconsumed bytes','surplus bytes'),
        ('too_many',w(2000001,5),'array exceeds','count/span budget'),
        ('huge_count',w(4294967295,5),'array exceeds','count/span budget'),
    ]
    for name, word in [('nan',0x7fc00001),('infinity',0x7f800000),('negative_infinity',0xff800000)]:
        cases.append((name,w(1,5,word)+bytes([255]),'nonfinite','nonfinite'))
    for tag in [0,1,2,3,4,6,4294967295]:
        cases.append(('tag-'+str(tag),w(1,tag,0)+bytes([255]),'key type '+str(tag)+' unsupported','key type unsupported'))
    for name, raw, rust_reason, native_reason in cases:
        path = malformed_inputs / f'malformed-{name}.blob'; path.write_bytes(source(34,'baseline',raw))
        native_code, _, native_report = native('malformed-'+name,path)
        code, report, stderr = rust('malformed-'+name,path)
        assert native_code and native_reason in json.dumps(native_report),name
        assert code and rust_reason in stderr + json.dumps(report),name
        old_native, old_path, _ = native('opaque-'+name,path,5)
        old_code, old_report, _ = rust('opaque-'+name,path,old_path,5)
        assert not old_native and not old_code and old_report['failures']==0,name
        malformed.append(dict(name=name,native_reason=native_reason,rust_reason=rust_reason))
    native_code, retail_path, retail = native('retail',args.retail_inputs)
    code, actual, _ = rust('retail',args.retail_inputs,retail_path)
    assert not native_code and not code and actual['failures']==0
    old = json.loads(args.frozen_schema5_oracle.read_text())
    assert len(retail['files']) == len(old['files']) == 70
    for fresh, frozen in zip(retail['files'],old['files']):
        assert fresh['file']==frozen['file']
        projected={key:value for key,value in fresh.items() if key!='bool_keys'}
        assert projected==frozen,'earlier source projection changed'
    count=sum(len(row['bool_keys']) for row in retail['files'])
    assert count==40
    audit = json.loads(args.layout_audit.read_text())
    for row in retail['files']:
        raw = [b for b in audit['blocks'] if b['file'] == row['file']]
        assert len(raw) == len(row['bool_keys'])
        for block, independent in zip(row['bool_keys'],raw):
            assert all(block[key]==independent[key] for key in ('block','block_type','offset','bytes','sha256'))
            assert block['data']==dict(declared_keys=independent['declared_keys'],key_type=independent['key_type'],keys=independent['keys'])
            assert independent['source_sha256']==row['sha256']
    summary=dict(authored_files=96,altered_reports=len(negatives),malformed_inputs=len(malformed),negatives=negatives,malformed=malformed,originals=dict(files=len(retail['files']),counts={"NiBoolData":count},keys=sum(b["data"]["declared_keys"] for r in retail["files"] for b in r["bool_keys"]),work=sum(r['bool_keys']['work_units'] for r in actual['files']),retained_bytes=sum(r['bool_keys']['retained_bytes'] for r in actual['files']),remaining_dependencies=actual['unresolved_dependencies'],diagnostics=actual['diagnostics']),runtime_ready=False,bool_evaluation=False,timeline_events_verified=False)
    assert original_hashes=={p.name:dict(bytes=p.stat().st_size,sha256=digest(p)) for p in args.retail_inputs.iterdir()}
    assert authored_hashes=={p.name:digest(p) for p in inputs.iterdir()}
    assert binaries==[digest(args.binary),digest(args.oracle)]
    write(args.output_dir / 'summary.json',summary)
    print(json.dumps(summary))

if __name__ == '__main__': main()
