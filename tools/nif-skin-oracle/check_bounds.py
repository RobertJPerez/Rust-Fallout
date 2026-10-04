"""Literal source-frame bounds through the real CLI and frozen prior poses.

The existing independent byte writer supplies a second authored source. Fixed
endpoints below do not implement skinning or compare against source-only spheres.
This checks CPU engineering output, not original playback or renderer culling.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_sampled as writer


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def bits(value):
    return struct.unpack('<Q', struct.pack('<d', value))[0]


def skin(influences):
    result = writer.transform(writer.ID, [1, -2, 3], .5) + writer.words(2) + bytes([1])
    for (rotation, translation, scale), rows in zip([
        (writer.ID, [-1, 0, 2], 1), (writer.R90, [0, -1, 0], 2),
    ], influences):
        result += writer.transform(rotation, translation, scale) + writer.floats(0, 0, 0, 10) + writer.shorts(len(rows))
        for vertex, weight in rows: result += writer.shorts(vertex) + writer.floats(weight)
    return result


def check(binary, prior_binary, output):
    binary = binary.resolve(); prior_binary = prior_binary.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = output / 'inputs'; inputs.mkdir()
    requests = output / 'requests'; requests.mkdir()
    source = inputs / 'authored.nif'; source.write_bytes(writer.container(writer.fixture()))
    frozen = {str(p): sha(p) for p in (binary, prior_binary, source)}
    invocations = []; literals = []; equal = []; refused = []; strict = []
    base = dict(schema_version=1, expected_source_sha256=list(bytes.fromhex(sha(source))),
                geometry=3, weights=dict(kind='require_unit_sum', absolute_tolerance=0),
                pose=dict(kind='stored'))

    def request_file(name, request):
        path = requests / (name + '.json'); path.write_text(json.dumps(request) + '\n')
        frozen[str(path)] = sha(path); return path

    def invoke(exe, name, args, code=0, destination=None, no_report=False):
        destination = destination or output / (name + '.json')
        command = [str(exe), *args, '--output', str(destination)]
        process = subprocess.run(command, capture_output=True, text=True)
        (output / (name + '.stderr.txt')).write_text(process.stderr)
        assert (process.returncode != 0 if no_report else process.returncode == code), (name, process.stderr)
        if no_report:
            assert not destination.exists(), name
            report = None
        else:
            report = json.loads(destination.read_text())
            assert report['failures'] == code, report
        invocations.append(dict(command=command, exit_code=process.returncode,
                                receipt_sha256=sha(destination) if destination.exists() else None))
        return report, destination

    def run(name, request, input_path=source, error=None):
        path = request_file(name, request)
        report, _ = invoke(binary, name, ['nif-skin', str(input_path), '--bounds-request', str(path)], int(error is not None))
        if error:
            assert report['evaluation'] is None and report['error'], report
            if error is not True: assert error in report['error'], report
            refused.append(name); return None
        result = report['evaluation']
        assert result['source_sha256'] == sha(input_path)
        assert (result['geometry'], result['geometry_data'], result['instance'], result['skin_data'], result['skeleton_root'], result['vertices']) == (3, 6, 4, 5, 0, 3)
        assert result['frame'] == 'existing-cpu-source-skin-root-binary64-positions'
        assert result['skin_to_source_world'] == [[0, -4, 0, -4], [4, 0, 0, -6], [0, 0, 4, -4]]
        assert result['retained_bytes'] == result['extra_retained_bytes'] + result['pose_retained_bytes']
        assert result['work_units'] == result['extra_work_units'] + result['pose_work_units']
        assert result['extra_work_units'] == 6 * input_path.stat().st_size + 27
        assert not result['retail_behavior_verified']
        return result

    # Noncommuting bind/rig transforms, negative coordinates, reflected/zero
    # sample scale and different source placement from the Rust fixture.
    cases = [
        ('stored', None, [-9, -1.5, .5], [6.5, 2.75, 11.375]),
        ('first', -2, [-9, -4, .5], [2.625, 3.375, 9.375]),
        ('zero-scale', 0, [-9, -2.5, .5], [2.75, 2.75, 10.375]),
        ('last', 2, [-9, -1.5, .5], [7.5, 2.125, 11.375]),
        ('negative-zero-time', -0.0, [-9, -2.5, .5], [2.75, 2.75, 10.375]),
    ]
    for name, time, minimum, maximum in cases:
        request = copy.deepcopy(base)
        if time is not None:
            request['pose'] = dict(kind='sampled', object=1, controller=7, source_time=time, controller_policy='refuse_other_required')
        result = run(name, request)
        assert result['coordinates'] == dict(min=minimum, max=maximum, min_f64_bits=list(map(bits, minimum)), max_f64_bits=list(map(bits, maximum))), result['coordinates']
        literals.append(name)
        if time is None:
            args = ['nif-skin', str(source), '--pose-geometry', '3', '--pose-weight-tolerance', '0']
            nested = None
        else:
            old_request = dict(schema_version=1, expected_source_sha256=base['expected_source_sha256'], geometry=3,
                               absolute_weight_tolerance=0, object=1, controller=7, source_time=time, controller_policy='refuse_other_required')
            old_path = request_file(name + '-prior-request', old_request)
            args = ['nif-skin', str(source), '--sampled-pose-request', str(old_path)]
            nested = 'sample'
        old, old_file = invoke(prior_binary, name + '-prior', args)
        current, current_file = invoke(binary, name + '-preserved', args)
        assert old_file.read_bytes() == current_file.read_bytes(), name
        prior_pose = old['evaluation']
        assert (result['pose_retained_bytes'], result['pose_work_units']) == (prior_pose['retained_bytes'], prior_pose['work_units'])
        old_skin = prior_pose['skin'] if nested else prior_pose
        assert result['unapplied_controllers'] == old_skin['unapplied_controllers']
        if nested:
            assert result['sample'] == prior_pose['sample']
            assert result['sample']['requested_time_f64_bits'] == bits(time)
        else:
            assert result['sample'] is None
        for point in old_skin['positions']:
            assert all(minimum[axis] <= point[axis] <= maximum[axis] for axis in range(3))
        equal.append(name)

    stale = copy.deepcopy(base); stale['expected_source_sha256'][0] ^= 1
    run('stale', stale, error='SHA256 differs')
    run('wrong-geometry', dict(base, geometry=1), error='no decoded skin owner')
    sampled = dict(base, pose=dict(kind='sampled', object=1, controller=7, source_time=0, controller_policy='refuse_other_required'))
    for name, patch in [('wrong-link', dict(controller=8)), ('before', dict(source_time=-3)), ('after', dict(source_time=3)), ('wrong-target', dict(object=2))]:
        run(name, dict(sampled, pose=dict(sampled['pose'], **patch)), error=True)

    def changed(name, blocks, request, error=None):
        path = inputs / (name + '.nif'); path.write_bytes(writer.container(blocks)); frozen[str(path)] = sha(path)
        return run(name, dict(request, expected_source_sha256=list(bytes.fromhex(sha(path)))), path, error)

    raw = dict(base, weights=dict(kind='preserve_raw_nonnegative'))
    blocks = writer.fixture(); blocks[5] = ('NiSkinData', skin([[(0, .25), (0, .5), (1, 1), (2, 0)], [(0, .75), (2, 1)]]))
    result = changed('raw-duplicate-nonunit-zero-term', blocks, raw)
    assert result['coordinates']['min'] == [-9, -1.5, .5]
    assert result['coordinates']['max'] == [6.5, 1.5, 15.375]
    literals.append('raw-duplicate-nonunit-zero-term')
    payload = bytearray(blocks[6][1]); payload[47] = 0; del payload[48:84]; blocks[6] = (blocks[6][0], bytes(payload))
    absent = changed('raw-absent-normals', blocks, raw)
    assert absent['coordinates'] == result['coordinates']
    literals.append('raw-absent-normals')
    blocks[5] = ('NiSkinData', skin([[(0, 1), (1, 1), (2, 0)], []]))
    changed('raw-zero-total', blocks, raw, 'no positive finite weight sum')

    blocks = writer.fixture(); payload = bytearray(blocks[2][1]); payload[8:12] = writer.words(7); blocks[2] = (blocks[2][0], bytes(payload))
    changed('other-required-controller', blocks, sampled, True)
    blocks = writer.fixture(); payload = bytearray(blocks[6][1]); payload[9:13] = writer.floats(float('inf')); blocks[6] = (blocks[6][0], bytes(payload))
    changed('nonfinite-source', blocks, base, True)
    blocks = writer.fixture(); payload = bytearray(blocks[6][1]); payload[8] = 0; del payload[9:45]; blocks[6] = (blocks[6][0], bytes(payload))
    changed('missing-positions', blocks, base, 'vertex positions unavailable')
    blocks = writer.fixture(); payload = bytearray(blocks[5][1]); payload[48:52] = writer.floats(0); blocks[5] = (blocks[5][0], bytes(payload))
    changed('singular-skin-transform', blocks, base, True)

    # Decode-only/schema errors must never create a successful or partial report.
    invalid = [dict(base, schema_version=2), dict(base, extra=1),
               dict(base, pose=dict(kind='stored', extra=1)),
               dict(base, weights=dict(kind='preserve_raw_nonnegative', extra=1)),
               dict(sampled, pose=dict(sampled['pose'], extra=1)),
               dict(base, weights=dict(kind='normalize')),
               dict(base, pose=dict(kind='sampled', object=1, controller=7, source_time=0)),
               dict(sampled, pose=dict(sampled['pose'], controller_policy='stored_fallback'))]
    missing = copy.deepcopy(base); del missing['weights']; invalid.append(missing)
    for ordinal, request in enumerate(invalid):
        name = f'strict-{ordinal}'
        path = request_file(name, request)
        invoke(binary, name, ['nif-skin', str(source), '--bounds-request', str(path)], no_report=True)
        strict.append(name)
    path = request_file('protected-request', base)
    for name, destination in [('protected-source', inputs / 'blocked.json'), ('protected-request', requests / 'blocked.json')]:
        invoke(binary, name, ['nif-skin', str(source), '--bounds-request', str(path)], destination=destination, no_report=True)
        strict.append(name)
    for path, digest in frozen.items(): assert sha(Path(path)) == digest, path
    result = dict(contract='engineering-source-bound-posed-skin-bounds-v1', literal_cases=literals,
                  prior_full_reports_byte_equal=equal, semantic_refusals=refused, strict_and_protected_refusals=strict,
                  invocations=invocations, frozen_hashes=frozen, retail_behavior_verified=False)
    (output / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--prior-binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    result = check(arguments.binary, arguments.prior_binary, arguments.output)
    print(json.dumps({k: v for k, v in result.items() if k not in ('invocations', 'frozen_hashes')}))
