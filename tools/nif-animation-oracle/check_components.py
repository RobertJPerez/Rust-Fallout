"""Schema4 exact compact float/point3 source comparisons and intended negatives."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

from component_fixtures import VARIANTS, expected, payloads, source
from fixtures import STREAMS, w
from process_guard import guard, run

def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def write(path, document):
    with path.open('x', encoding='utf-8') as output:
        json.dump(document, output, indent=2)
        output.write('\n')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--oracle', type=Path, required=True)
    parser.add_argument('--retail-inputs', type=Path)
    parser.add_argument('--frozen-schema3-oracle', type=Path)
    args = parser.parse_args()
    guard()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    inputs = args.output_dir / 'inputs'
    inputs.mkdir()
    for stream in STREAMS:
        for variant in VARIANTS:
            (inputs / f'stream-{stream}-{variant}.blob').write_bytes(source(stream, variant))
    original_hashes = {p.name: dict(bytes=p.stat().st_size, sha256=digest(p)) for p in args.retail_inputs.iterdir()} if args.retail_inputs else None
    if original_hashes: write(args.output_dir / 'original-inputs-before.json', original_hashes)
    authored_hashes = {p.name: digest(p) for p in inputs.iterdir()}
    binaries = [digest(args.binary), digest(args.oracle)]
    flags = {1: [], 2: ['--include-keyframes'], 3: ['--include-splines'], 4: ['--include-spline-components']}

    def native(name, directory, schema=4):
        path = args.output_dir / f'{name}-oracle.json'
        with path.open('xb') as output:
            result = run([str(args.oracle.resolve()), str(directory.resolve()), *flags[schema]], stdout=output, stderr=subprocess.PIPE)
        (args.output_dir / f'{name}-native.stderr.txt').write_bytes(result.stderr)
        document = json.loads(path.read_text(encoding='utf-8'))
        assert document['oracle_binary_sha256'] == binaries[1]
        return result.returncode, path, document

    def rust(name, directory, oracle=None, schema=4):
        path = args.output_dir / f'{name}-rust.json'
        command = [str(args.binary.resolve()), 'nif-animation', str(directory.resolve()), '--output', str(path.resolve()), *flags[schema]]
        if oracle: command += ['--oracle-report', str(oracle.resolve())]
        result = run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        (args.output_dir / f'{name}-rust.stdout.txt').write_bytes(result.stdout)
        (args.output_dir / f'{name}-rust.stderr.txt').write_bytes(result.stderr)
        document = json.loads(path.read_text()) if path.exists() else None
        return result.returncode, path, document, result.stderr.decode('utf-8', errors='replace')

    native_code, path, oracle = native('authored', inputs)
    code, _, document, _ = rust('authored', inputs, path)
    assert not native_code and not code, 'authored native/Rust comparison failed; evidence retained'
    for row in oracle['files']:
        variant = row['file'].split('-', 2)[2][:-5]
        assert [b['data'] for b in row['spline_components']] == expected(variant), row['file']
    assert len(oracle['files']) == len(STREAMS) * len(VARIANTS) and not document['runtime_ready']
    def row(d): return next(r for r in d['files'] if r['file'] == 'stream-34-baseline.blob')
    def block(d, index=0): return row(d)['spline_components'][index]
    def field(d, index=0): return block(d, index)['data']
    changes = {
        'block': lambda d: block(d).update(block=0),
        'type': lambda d: block(d).update(block_type='NiBSplineFloatInterpolator'),
        'offset': lambda d: block(d).update(offset=0),
        'bytes': lambda d: block(d).update(bytes=0),
        'hash': lambda d: block(d).update(sha256='0' * 64),
        'scalar_bits': lambda d: field(d).update(value_bits=0),
        'vector_bits': lambda d: field(d, 1)['value_bits'].__setitem__(1, 0),
        'vector_order': lambda d: field(d, 1)['value_bits'].reverse(),
        'handle': lambda d: field(d).update(handle=4294967295),
        'time': lambda d: field(d).update(start_bits=0),
        'data_link': lambda d: field(d).update(spline_data=11),
        'basis_link': lambda d: field(d).update(basis_data=10),
        'offset_bits': lambda d: field(d).update(float_offset_bits=0),
        'half_range_bits': lambda d: field(d, 1).update(position_half_range_bits=0),
        'extra_field': lambda d: field(d).update(evaluated_pose=True),
        'omission': lambda d: row(d)['spline_components'].pop(),
        'branch': lambda d: d.update(spline_component_branch='other'),
        'raw_contract': lambda d: d.update(raw_component_fields_checked=False),
        'schema': lambda d: d.update(schema_version=3),
    }
    negatives = []
    for name, mutate in changes.items():
        guard()
        changed = copy.deepcopy(oracle)
        mutate(changed)
        path = args.output_dir / f'altered-{name}.json'
        write(path, changed)
        code, _, document, stderr = rust(name, inputs, path)
        reason = 'provenance contract' if name in ('branch', 'raw_contract', 'schema') else 'oracle spline-component block identity/span/hash or source fields differ'
        assert code and reason in stderr + json.dumps(document), name
        negatives.append(dict(name=name, exit=code, intended_reason=reason))
    scalar, vector = payloads('baseline')
    malformed = {}
    for kind, raw in [('scalar', scalar), ('vector', vector)]:
        malformed[f'truncated-{kind}'] = ({kind: raw[:-1]}, 'source span differs', 'field exceeds')
        malformed[f'surplus-{kind}'] = ({kind: raw + b'\0'}, 'source span differs', 'unconsumed bytes')
        for suffix, offset, native_reason, rust_reason in [
            ('nan-value', 16, 'nonfinite', 'nonfinite'),
            ('nan-half-range', len(raw) - 4, 'nonfinite', 'nonfinite'),
            ('bad-data-link', 8, 'link out of range', 'block index out of range'),
            ('wrong-data-kind', 8, 'wrong target kind', 'wrong target kind'),
            ('wrong-basis-kind', 12, 'wrong target kind', 'wrong target kind'),
        ]:
            word = 0x7fc00001 if suffix.startswith('nan') else 99 if suffix == 'bad-data-link' else 11 if suffix == 'wrong-data-kind' else 10
            malformed[f'{suffix}-{kind}'] = ({kind: raw[:offset] + w(word) + raw[offset + 4:]}, native_reason, rust_reason)
    for name, (replacements, native_reason, rust_reason) in malformed.items():
        guard()
        directory = args.output_dir / f'bad-{name}'
        directory.mkdir()
        (directory / 'invalid.blob').write_bytes(source(34, 'baseline', replacements))
        native_code, _, native_doc = native(f'bad-{name}', directory)
        code, _, rust_doc, _ = rust(f'bad-{name}', directory)
        assert native_code and code and native_reason in native_doc['files'][0]['error'] and rust_reason in rust_doc['files'][0]['error'], (name, native_doc, rust_doc)
        for schema in (1, 2, 3):
            old_code, old_path, _ = native(f'opaque-native-{schema}-{name}', directory, schema)
            rust_code, _, _, _ = rust(f'opaque-{schema}-{name}', directory, old_path, schema)
            assert not old_code and not rust_code, (name, schema)
        negatives.append(dict(name=f'malformed-{name}', native_exit=native_code, rust_exit=code, intended_reason=rust_reason))
    regression = []
    for schema in (1, 2, 3):
        native_code, old_path, old_doc = native(f'schema{schema}', inputs, schema)
        code, _, _, _ = rust(f'schema{schema}', inputs, old_path, schema)
        assert not native_code and not code, schema
        for old, new in zip(old_doc['files'], oracle['files']):
            omitted = ('keys', 'splines', 'spline_components') if schema == 1 else ('splines', 'spline_components') if schema == 2 else ('spline_components',)
            assert old == {k: v for k, v in new.items() if k not in omitted}
        regression.append(schema)
    originals = None
    if args.retail_inputs:
        native_code, original_path, original_doc = native('retail', args.retail_inputs)
        code, _, original_rust, _ = rust('retail', args.retail_inputs, original_path)
        assert not native_code and not code
        counts = {}
        for file in original_doc['files']:
            for block in file['spline_components']: counts[block['block_type']] = counts.get(block['block_type'], 0) + 1
        originals = dict(files=len(original_doc['files']), counts=counts, work_units=sum(f['spline_components']['work_units'] for f in original_rust['files']), retained_bytes=sum(f['spline_components']['retained_bytes'] for f in original_rust['files']), unresolved_dependencies=original_rust['unresolved_dependencies'], diagnostics=original_rust['diagnostics'], runtime_ready=original_rust['runtime_ready'])
        if args.frozen_schema3_oracle:
            native_code, _, old_doc = native('retail-schema3', args.retail_inputs, 3)
            frozen = json.loads(args.frozen_schema3_oracle.read_text())
            assert not native_code and {k:v for k,v in old_doc.items() if k != 'oracle_binary_sha256'} == {k:v for k,v in frozen.items() if k != 'oracle_binary_sha256'}
            code, _, _, _ = rust('retail-frozen-schema3', args.retail_inputs, args.frozen_schema3_oracle, 3)
            assert not code
        after = {p.name:dict(bytes=p.stat().st_size, sha256=digest(p)) for p in args.retail_inputs.iterdir()}
        write(args.output_dir / 'original-inputs-after.json', after)
        assert after == original_hashes
    assert authored_hashes == {p.name:digest(p) for p in inputs.iterdir()}
    assert binaries == [digest(args.binary), digest(args.oracle)]
    summary = dict(authored_files=len(oracle['files']), streams=list(STREAMS), altered_reports=len(changes), malformed_inputs=len(malformed), negatives=negatives, earlier_schema_projections_exact=regression, originals=originals, binaries_sha256=binaries, runtime_ready=False, numerical_evaluation=False)
    write(args.output_dir / 'summary.json', summary)
    print(json.dumps(summary))

if __name__ == '__main__': main()
