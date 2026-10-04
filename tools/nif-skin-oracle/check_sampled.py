"""Second authored source with literal numeric expectations, using the real CLI.

No Rust pose helpers or retail interpolation/playback assumptions enter these
expectations. Original inputs, native receipts and frozen binaries stay local.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

NULL = 0xffffffff
ID = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
R90 = [[0, -1, 0], [1, 0, 0], [0, 0, 1]]
RM90 = [[0, 1, 0], [-1, 0, 0], [0, 0, 1]]

def words(*values):
    return struct.pack('<' + 'I' * len(values), *values)

def floats(*values):
    return struct.pack('<' + 'f' * len(values), *values)

def shorts(*values):
    return struct.pack('<' + 'H' * len(values), *values)

def av(rotation, translation, scale, controller=NULL):
    return words(NULL, 0, controller, 0) + floats(*translation) + floats(*sum(rotation, []), scale) + words(0, NULL)

def node(rotation, translation, scale, children, controller=NULL):
    return av(rotation, translation, scale, controller) + words(len(children), *children, 0)

def transform(rotation, translation, scale):
    return floats(*sum(rotation, []), *translation, scale)

def fixture():
    geometry = av(ID, [999, 888, 777], 99) + words(6, 4, 0, NULL) + bytes([0])
    data = words(0) + shorts(3) + bytes([0, 0, 1])
    for point in [[2, -1, 3], [-2, 4, 1], [0, 3, -1]]:
        data += floats(*point)
    data += shorts(0) + bytes([1]) + floats(*([1, 2, 0] * 3))
    data += floats(0, 0, 0, 10) + bytes([0]) + shorts(0) + words(NULL)
    data += shorts(1) + words(3) + bytes([1]) + shorts(0, 1, 2, 0)
    skin = transform(ID, [1, -2, 3], .5) + words(2) + bytes([1])
    for rotation, translation, scale, influences in [
        (ID, [-1, 0, 2], 1, [(0, .25), (1, 1)]),
        (R90, [0, -1, 0], 2, [(0, .75), (2, 1)]),
    ]:
        skin += transform(rotation, translation, scale) + floats(0, 0, 0, 10) + shorts(len(influences))
        for vertex, weight in influences:
            skin += shorts(vertex) + floats(weight)
    controller = words(NULL) + shorts(0xffff) + floats(17, -9, 100, 101) + words(1, 8)
    interpolator = floats(1000, 2000, 3000, 2, -3, 4, -5, 12) + words(9)
    keys = words(0, 2, 1) + floats(-2, -1, 2, 4, 2, 5, -4, 0)
    keys += words(2, 1) + floats(-2, -2, 2, 2)
    return [
        ('NiNode', node(R90, [4, -2, 8], 2, [1, 2, 3])),
        ('NiNode', node(RM90, [3, 1, 0], 2, [], 7)),
        ('NiNode', node(ID, [-2, 4, 1], 3, [])),
        ('NiTriShape', geometry),
        ('NiSkinInstance', words(5, NULL, 0, 2, 1, 2)),
        ('NiSkinData', skin), ('NiTriShapeData', data),
        ('NiTransformController', controller),
        ('NiTransformInterpolator', interpolator), ('NiTransformData', keys),
    ]

def container(blocks):
    types = list(dict.fromkeys(name for name, _ in blocks))
    result = b'Gamebryo File Format, Version 20.2.0.7\n' + words(0x14020007) + bytes([1])
    result += words(11, len(blocks), 34) + bytes(3) + shorts(len(types))
    for name in types:
        result += words(len(name)) + name.encode('ascii')
    result += shorts(*(types.index(name) for name, _ in blocks))
    result += words(*(len(payload) for _, payload in blocks)) + words(0, 0, 0)
    return result + b''.join(payload for _, payload in blocks) + words(1, 0)

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def check(binary, output):
    binary = binary.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = output / 'inputs'
    inputs.mkdir()
    requests = output / 'requests'
    requests.mkdir()
    source = inputs / 'authored.nif'
    source.write_bytes(container(fixture()))
    original = {str(path): sha(path) for path in [binary, source]}
    base = dict(schema_version=1, expected_source_sha256=list(bytes.fromhex(sha(source))), geometry=3,
                absolute_weight_tolerance=0, object=1, controller=7, source_time=0,
                controller_policy='refuse_other_required')
    invocations = []
    def run(name, request, error=None, input_path=source, destination=None):
        path = requests / (name + '.json')
        path.write_text(json.dumps(request) + '\n')
        original[str(path)] = sha(path)
        destination = destination or output / (name + '.json')
        command = [str(binary), 'nif-skin', str(input_path), '--sampled-pose-request', str(path), '--output', str(destination)]
        process = subprocess.run(command, capture_output=True, text=True)
        if error in ('protected', 'schema'):
            assert process.returncode != 0 and not destination.exists(), process.stdout + process.stderr
        else:
            report = json.loads(destination.read_text())
            assert process.returncode == (0 if error is None else 1), process.stderr
            assert report['failures'] == (0 if error is None else 1)
            if error:
                assert report['evaluation'] is None and error in report['error'], report
            else:
                value = report['evaluation']
                assert value['sample']['requested_time_f64_bits'] == struct.unpack('<Q', struct.pack('<d', request['source_time']))[0]
                assert value['skin']['source_sha256'] == value['sample']['source_sha256'] == sha(input_path)
                assert not value['retail_behavior_verified'] and not value['skin']['retail_behavior_verified']
                assert value['skin']['unapplied_controllers'] == []
        invocations.append(dict(command=command, exit_code=process.returncode, expected_error=error,
                                receipt_sha256=sha(destination) if destination.exists() else None))
        return None if error else report['evaluation']
    cases = [
        ('first', -2, [[2.625, 3.375, 9.375], [-3.5, -4, 2], [-9, -1.5, .5]], [-5, 2.5, 0], [-2, 1, 0]),
        ('zero-scale', 0, [[2.75, 2.75, 10.375], [2, -2.5, 4], [-9, -1.5, .5]], [-4.5, 2.25, 0], [0, 0, 0]),
        ('last', 2, [[2.875, 2.125, 11.375], [7.5, -1, 6], [-9, -1.5, .5]], [-4, 2, 0], [2, -1, 0]),
    ]
    for name, time, positions, mixed_normal, selected_normal in cases:
        value = run(name, dict(base, source_time=time))
        assert value['skin']['positions'] == positions, value['skin']['positions']
        assert value['skin']['normals'] == [mixed_normal, selected_normal, [-6, 3, 0]]
        assert value['skin']['palette'][1]['matrix'] == [[0, -3, 0, 0], [3, 0, 0, -1.5], [0, 0, 3, 3.5]]
        assert value['skin']['skin_to_source_world'] == [[0, -4, 0, -4], [4, 0, 0, -6], [0, 0, 4, -4]]
        assert value['skin']['weight_sums'] == [1, 1, 1]
    stale = base['expected_source_sha256'].copy()
    stale[0] ^= 1
    run('stale', dict(base, expected_source_sha256=stale), 'source SHA256 differs')
    run('link', dict(base, controller=8), 'object.controller differs')
    run('before', dict(base, source_time=-3), 'extrapolate')
    run('after', dict(base, source_time=3), 'extrapolate')
    run('geometry', dict(base, geometry=1), 'selected geometry has no decoded skin owner')
    run('strict-schema', dict(base, extra=1), 'schema')
    run('explicit-policy', dict(base, controller_policy='stored_fallback'), 'schema')
    run('protected-request', base.copy(), 'protected', destination=requests / 'blocked-output.json')
    run('protected-source', base.copy(), 'protected', destination=inputs / 'blocked-output.json')
    for path, digest in original.items():
        assert sha(Path(path)) == digest
    result = dict(contract='engineering-one-linked-sample-skin-v1', analytic_cases=3, intended_refusals=9,
                  invocations=invocations, frozen_hashes=original, retail_behavior_verified=False)
    (output / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    return result

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    result = check(arguments.binary, arguments.output)
    print(json.dumps({key: value for key, value in result.items() if key not in ('invocations', 'frozen_hashes')}))
