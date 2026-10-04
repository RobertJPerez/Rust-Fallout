"""Physical marker literals through the indexed CLI; no event/playback inference."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import fixtures as w

KEYS = [(2, 1), (-0.0, 2), (1, 3), (1, 1), (-2, 4), (0.0, 2), (3, 5)]
STRINGS = [b'sequence', b'dup', b'\xff\0\0', b'', b'early', b'late']

def fixture(keys=KEYS, link=1):
    sequence = w.w(0, 0, 17) + w.f(.5) + w.w(link, 37) + w.f(7, 100, 101) + w.w(w.N, w.N) + w.h(0)
    text = w.w(w.N, len(keys)) + b''.join(w.f(time) + w.w(string) for time, string in keys)
    # An unlinked missing string must never enter the selected index.
    other = w.w(w.N, 1) + w.f(99) + w.w(w.N)
    return w.container(34, [('NiControllerSequence', sequence), ('NiTextKeyExtraData', text),
                            ('NiTextKeyExtraData', other)], STRINGS)

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def semantic(value):
    return {key: item for key, item in value.items() if key not in
            ('retained_bytes', 'decoded_source_retained_bytes', 'combined_retained_bytes', 'work_units')}

def check(binary, prior, output):
    binary, prior = binary.resolve(), prior.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs, requests = output/'inputs', output/'requests'; inputs.mkdir(); requests.mkdir()
    source = inputs/'authored.kf'; source.write_bytes(fixture())
    frozen = {str(path): sha(path) for path in (binary, prior, source)}; invocations = []
    intervals = [dict(source_start=start, source_end=end) for start, end in
                 [(0, 2), (1, 1), (0.0, -0.0), (-9, 9), (4, 5), (2, 2), (-2, -2)]]
    expected_ordinals = [[0,1,2,3,5], [2,3], [1,5], [0,1,2,3,4,5,6], [], [0], [4]]
    base = dict(schema_version=1, expected_sha256=list(bytes.fromhex(sha(source))), sequence=0, intervals=intervals)
    def invoke(name, request, source_path=source, error=None, destination=None, executable=binary,
               mode='--prepared-markers-request', flags=(), expected_exit=None):
        path = requests/(name+'.json'); path.write_text(json.dumps(request)+'\n'); frozen[str(path)] = sha(path)
        dest = destination or output/(name+'.json')
        command = [str(executable), 'nif-animation', str(source_path), mode, str(path), *flags, '--output', str(dest)]
        process = subprocess.run(command, capture_output=True, text=True)
        (output/(name+'.stderr.txt')).write_text(process.stderr)
        code = expected_exit if expected_exit is not None else (1 if error else 0)
        assert process.returncode == code, (name, process.stdout, process.stderr)
        if error in ('schema', 'protected', 'conflict'):
            assert not dest.exists(), name
            result = None
        else:
            report = json.loads(dest.read_text()); assert report['failures'] == code
            result = report['evaluation']
            if error: assert result is None and error in report['error'], report
            else: assert report['error'] is None
        invocations.append(dict(case=name, command=command, exit_code=process.returncode,
                                receipt_sha256=sha(dest) if dest.exists() else None, expected_error=error))
        return result, dest
    batch, _ = invoke('indexed', base)
    assert batch['preparation']['animation_decodes'] == batch['preparation']['full_source_sha256_traversals'] == 1
    assert batch['preparation']['validated_keys'] == 7
    assert not batch['retail_behavior_verified']
    for ordinal, (result, expected) in enumerate(zip(batch['intervals'], expected_ordinals, strict=True)):
        value, usage = result['observation'], result['usage']
        assert [e['source_key_ordinal'] for e in value['entries']] == expected
        literal = [dict(source_key_ordinal=i, time_bits=struct.unpack('<I', w.f(KEYS[i][0]))[0],
                        string_index=KEYS[i][1], raw_string_bytes=list(STRINGS[KEYS[i][1]])) for i in expected]
        assert value['entries'] == literal
        assert value['source_sha256'] == sha(source)
        assert value['source_start_f64_bits'] == struct.unpack('<Q', struct.pack('<d', intervals[ordinal]['source_start']))[0]
        assert value['source_end_f64_bits'] == struct.unpack('<Q', struct.pack('<d', intervals[ordinal]['source_end']))[0]
        assert value['sequence']['block'] == 0 and value['text_keys']['block'] == 1
        assert value['unapplied_sequence_clock_fields'] == dict(cycle_type=37, frequency_bits=0x40e00000,
              start_bits=0x42c80000, stop_bits=0x42ca0000, weight_bits=0x3f000000)
        assert value['decoded_source_retained_bytes'] == 0
        assert usage['animation_decodes'] == usage['full_source_sha256_traversals'] == usage['full_key_validations'] == 0
        assert usage['matching_index_visits'] == len(expected) * 2
        assert usage['combined_retained_bytes'] == usage['prepared_retained_bytes'] + usage['charged_bytes']
        assert not value['retail_behavior_verified']
        old_request = dict(schema_version=1, expected_sha256=base['expected_sha256'], sequence=0, **intervals[ordinal])
        old, old_path = invoke('old-'+str(ordinal), old_request, executable=prior, mode='--markers-request')
        current, current_path = invoke('current-old-'+str(ordinal), old_request, mode='--markers-request')
        assert old_path.read_bytes() == current_path.read_bytes()
        assert semantic(value) == semantic(old)
    permutation = [6, 2, 0, 4, 1, 5, 3]
    perm, _ = invoke('permuted', dict(base, intervals=[intervals[i] for i in permutation]))
    assert perm['intervals'] == [batch['intervals'][i] for i in permutation]
    repeats, _ = invoke('repeated-64', dict(base, intervals=[intervals[2]]*64))
    assert repeats['intervals'] == [batch['intervals'][2]]*64
    stale = base['expected_sha256'].copy(); stale[0] ^= 1
    negatives = [('stale', dict(base, expected_sha256=stale), 'SHA256 differs'),
                 ('wrong-sequence', dict(base, sequence=1), 'not decoded NiControllerSequence'),
                 ('late-reversed', dict(base, intervals=intervals+[dict(source_start=8, source_end=7)]), 'finite ordered bounds'),
                 ('empty', dict(base, intervals=[]), 'schema'), ('too-many', dict(base, intervals=intervals*10), 'schema'),
                 ('schema', dict(base, schema_version=2), 'schema'), ('extra-root', dict(base, extra=1), 'schema'),
                 ('extra-interval', dict(base, intervals=[dict(intervals[0], extra=1)]), 'schema'),
                 ('missing-interval-field', dict(base, intervals=[dict(source_start=0)]), 'schema'),
                 ('null-intervals', dict(base, intervals=None), 'schema'),
                 ('wrong-time-type', dict(base, intervals=[dict(source_start='0', source_end=1)]), 'schema')]
    for name, request, error in negatives: invoke(name, request, error=error)
    for name, keys, link, error in [('absent-link', KEYS, w.N, 'has no text_keys link'),
                                    ('outside-null-string', [(99,w.N)], 1, 'has no authored string'),
                                    ('outside-nonfinite-time', [(float('inf'),1)], 1, 'nonfinite NIF float'),
                                    ('bad-string-index', [(99,99)], 1, 'string index out of range')]:
        changed = inputs/(name+'.kf'); changed.write_bytes(fixture(keys, link)); frozen[str(changed)] = sha(changed)
        invoke(name, dict(base, expected_sha256=list(bytes.fromhex(sha(changed)))), source_path=changed, error=error)
    empty = inputs/'empty.kf'; empty.write_bytes(fixture([])); frozen[str(empty)] = sha(empty)
    empty_batch, _ = invoke('authored-empty-array', dict(base, expected_sha256=list(bytes.fromhex(sha(empty)))), source_path=empty)
    assert empty_batch['preparation']['validated_keys'] == 0
    assert all(v['observation']['entries'] == [] and v['usage']['boundary_probes'] == 0 for v in empty_batch['intervals'])
    invoke('protected-source', base, error='protected', destination=inputs/'blocked.json')
    invoke('protected-request', base, error='protected', destination=requests/'blocked.json')
    modes = [('--markers-request', 'unused.json'), ('--oracle-report', 'unused.json'), ('--include-keyframes',),
             ('--include-splines',), ('--include-spline-components',), ('--include-bool-interpolators',),
             ('--include-bool-keys',), ('--sample-time', '0'), ('--sample-block', '0'), ('--sample-channel', 'translation')]
    for ordinal, flags in enumerate(modes):
        invoke('conflict-'+str(ordinal), base, flags=flags, error='conflict', expected_exit=2)
    after, _ = invoke('after-refusals', base); assert after == batch
    for path, digest in frozen.items(): assert sha(Path(path)) == digest
    summary = dict(contract='engineering-prepared-marker-index-consumer-v1', authored_intervals=7,
                   whole_old_reports_byte_equal=7, batch_schedules=3, intended_refusals=len(negatives)+4+2+len(modes),
                   invocations=invocations, frozen_hashes=frozen, retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
    return summary

if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--prior', type=Path, required=True); parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(); result = check(args.binary, args.prior, args.output)
    print(json.dumps({k:v for k,v in result.items() if k not in ('invocations','frozen_hashes')}))
