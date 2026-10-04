"""Second authored complete skin forest with literal matrices and vertices.

Only the existing public source writer is reused. Expectations are fixed numeric
values, not an importer, sampler or copy of the production deformation loop.
This proves engineering composition, not measured original animation playback.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_sampled as writer

NULL = writer.NULL


def keys(first, last, scales):
    return (writer.words(0, 2, 1) + writer.floats(-2, *first, 2, *last)
            + writer.words(2, 1) + writer.floats(-2, scales[0], 2, scales[1]))


def controller(target, interpolator):
    return (writer.words(NULL) + writer.shorts(0x004c)
            + writer.floats(17, -9, 100, 101) + writer.words(target, interpolator))


def interpolator(data):
    return writer.floats(1000, 2000, 3000, 2, -3, 4, -5, 12) + writer.words(data)


def skin(first_weight=.25, scale=.5):
    result = writer.transform(writer.ID, [1, -2, 3], scale) + writer.words(2) + bytes([1])
    for rotation, translation, bone_scale, influences in [
        (writer.ID, [-1, 0, 2], 1, [(0, first_weight), (1, 1)]),
        (writer.R90, [0, -1, 0], 2, [(0, .75), (2, 1)]),
    ]:
        result += (writer.transform(rotation, translation, bone_scale)
                   + writer.floats(0, 0, 0, 10) + writer.shorts(len(influences)))
        for vertex, weight in influences:
            result += writer.shorts(vertex) + writer.floats(weight)
    return result


def fixture():
    return [
        ('NiNode', writer.node(writer.R90, [1, -3, 2], 3, [], 8)),
        ('NiTriShapeData', writer.fixture()[6][1]),
        ('NiSkinData', skin()),
        ('NiTriShape', writer.av(writer.ID, [999, 888, 777], 99)
         + writer.words(1, 4, 0, NULL) + bytes([0])),
        ('NiSkinInstance', writer.words(2, NULL, 11, 2, 7, 0)),
        ('NiTransformData', keys([2, 1, -1], [-2, 5, 3], [1, 3])),
        ('NiTransformInterpolator', interpolator(5)),
        ('NiNode', writer.node(writer.RM90, [3, 1, 0], 2, [0], 9)),
        ('NiTransformController', controller(0, 6)),
        ('NiTransformController', controller(7, 10)),
        ('NiTransformInterpolator', interpolator(12)),
        ('NiNode', writer.node(writer.R90, [4, -2, 8], 2, [7, 3], 13)),
        ('NiTransformData', keys([-1, 2, 4], [3, -2, 0], [2, 4])),
        ('NiTransformController', controller(11, 15)),
        ('NiNode', writer.node(writer.RM90, [-5, 3, 9], 2, [11])),
        ('NiTransformInterpolator', interpolator(16)),
        ('NiTransformData', keys([1, -2, 4], [5, 2, -4], [1, -1])),
    ]


def container(blocks, roots=(14,)):
    return writer.container(blocks)[:-8] + writer.words(len(roots), *roots)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def channels(child, root, parent):
    return [dict(object=0, controller=8, source_time=child),
            dict(object=11, controller=13, source_time=root),
            dict(object=7, controller=9, source_time=parent)]


CASES = [
    ('first', channels(-2, -2, -2),
     [[[0, 1, 0, .5], [-1, 0, 0, 0], [0, 0, 1, 7]],
      [[0, -2, 0, 1.5], [2, 0, 0, -4], [0, 0, 2, 4]]],
     [[2.5, -.5, 10], [4.5, 2, 8], [-4.5, -4, 2]],
     [[-2.5, 1.25, 0], [2, -1, 0], [-4, 2, 0]],
     [[4, 0, 0, -13], [0, 4, 0, 9], [0, 0, 4, 5]],
     [[[4, 0, 0, -7], [0, 4, 0, -3], [0, 0, 4, 21]],
      [[2, 0, 0, -9], [0, 2, 0, 1], [0, 0, 2, 17]],
      [[0, 4, 0, -11], [-4, 0, 0, 5], [0, 0, 4, 25]]]),
    ('zero-root', channels(0, 0, 0),
     [[[0, 1.5, 0, 1.5], [-1.5, 0, 0, -.5], [0, 0, 1.5, 7]],
      [[0, -6, 0, 6], [6, 0, 0, -5], [0, 0, 6, 5.5]]],
     [[9, 4.375, 20.5], [7.5, 2.5, 8.5], [-12, -5, -.5]],
     [[-8.25, 4.125, 0], [3, -1.5, 0], [-12, 6, 0]],
     [[0, 0, 0, -5], [0, 0, 0, -3], [0, 0, 0, 9]],
     [[[0, 0, 0, -5], [0, 0, 0, -3], [0, 0, 0, 9]]] * 3),
    ('reflected-root', channels(2, 2, 2),
     [[[0, 2, 0, 2.5], [-2, 0, 0, -1], [0, 0, 2, 7]],
      [[0, -12, 0, 12.5], [12, 0, 0, -5], [0, 0, 12, 9]]],
     [[18.5, 13, 37], [10.5, 3, 9], [-23.5, -5, -3]],
     [[-17, 8.5, 0], [4, -2, 0], [-24, 12, 0]],
     [[-4, 0, 0, 3], [0, -4, 0, -15], [0, 0, -4, 13]],
     [[[-24, 0, 0, -47], [0, -24, 0, -19], [0, 0, -24, -23]],
      [[-2, 0, 0, -1], [0, -2, 0, -7], [0, 0, -2, 1]],
      [[0, -8, 0, -7], [8, 0, 0, -3], [0, 0, -8, 1]]]),
    ('mixed-times', channels(2, -2, 0),
     [[[0, 1.5, 0, 1.5], [-1.5, 0, 0, -.5], [0, 0, 1.5, 7]],
      [[0, -9, 0, 9], [9, 0, 0, -3.5], [0, 0, 9, 8.5]]],
     [[13.5, 10, 29.5], [7.5, 2.5, 8.5], [-18, -3.5, -.5]],
     [[-12.75, 6.375, 0], [3, -1.5, 0], [-18, 9, 0]],
     [[4, 0, 0, -13], [0, 4, 0, 9], [0, 0, 4, 5]],
     [[[18, 0, 0, 23], [0, 18, 0, 13], [0, 0, 18, 39]],
      [[2, 0, 0, -9], [0, 2, 0, 1], [0, 0, 2, 17]],
      [[0, 6, 0, -7], [-6, 0, 0, 1], [0, 0, 6, 21]]]),
]


def check(binary, prior_binary, output):
    binary = binary.resolve(); prior_binary = prior_binary.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = output / 'inputs'; inputs.mkdir()
    requests = output / 'requests'; requests.mkdir()
    blocks = fixture(); source = inputs / 'authored.nif'
    source.write_bytes(container(blocks))
    frozen = {str(p): sha(p) for p in [source, binary, prior_binary]}
    base = dict(schema_version=1, expected_source_sha256=list(bytes.fromhex(sha(source))),
                geometry=3, absolute_weight_tolerance=0,
                controller_policy='require_exact_required_forest')
    invocations = []; equal = []; observations = []; refusals = []

    def invoke(exe, name, args, expected_code=0):
        destination = output / (name + '.json')
        command = [str(exe), *args, '--output', str(destination)]
        process = subprocess.run(command, capture_output=True, text=True)
        (output / (name + '.stderr.txt')).write_text(process.stderr)
        assert process.returncode == expected_code, (name, process.stderr)
        report = json.loads(destination.read_text())
        assert report['failures'] == expected_code, report
        invocations.append(dict(command=command, exit_code=process.returncode, receipt_sha256=sha(destination)))
        return report, destination

    def run(name, selected, error=None, input_path=source, patch=None):
        request = dict(base, expected_source_sha256=list(bytes.fromhex(sha(input_path))), channels=selected)
        if patch: request.update(patch)
        path = requests / (name + '.json'); path.write_text(json.dumps(request) + '\n')
        frozen[str(path)] = sha(path)
        report, _ = invoke(binary, name, ['nif-skin', str(input_path), '--pose-set-request', str(path)], int(error is not None))
        if error:
            assert report['evaluation'] is None and report['error'], report
            if error is not True: assert error in report['error'], report
            refusals.append(name); return None
        value = report['evaluation']; pose = value['pose_set']; deformed = value['skin']
        assert pose['source_sha256'] == deformed['source_sha256'] == sha(input_path)
        assert [o['channel']['object']['block'] for o in pose['objects']] == [r['object'] for r in selected]
        assert [o['channel']['requested_time_f64_bits'] for o in pose['objects']] == [struct.unpack('<Q', struct.pack('<d', r['source_time']))[0] for r in selected]
        assert value['skin_scene_decodes'] == 1 and pose['preparation']['scene_decodes'] == 0
        assert pose['preparation']['animation_key_decodes'] == 1 and pose['propagated_objects'] == 5
        assert not value['retail_behavior_verified'] and not pose['retail_behavior_verified'] and not deformed['retail_behavior_verified']
        assert deformed['unapplied_controllers'] == [] and deformed['weight_sums'] == [1, 1, 1]
        return value

    for name, selected, palette, positions, normals, frame, worlds in CASES:
        value = run(name, selected); deformed = value['skin']; pose = value['pose_set']
        assert [p['node'] for p in deformed['palette']] == [7, 0]
        assert [p['matrix'] for p in deformed['palette']] == palette, (name, deformed['palette'])
        assert deformed['positions'] == positions, (name, deformed['positions'])
        assert deformed['normals'] == normals, (name, deformed['normals'])
        assert deformed['skin_to_source_world'] == frame, (name, deformed['skin_to_source_world'])
        assert [o['source_world'] for o in pose['objects']] == worlds, (name, pose['objects'])
        for item in pose['objects']:
            for span in [item['channel'][k] for k in ['object', 'controller', 'interpolator', 'data']] + [a['source'] for a in item['ancestors']]:
                offset = len(source.read_bytes()) - 8 - sum(len(p) for _, p in blocks[span['block']:])
                payload = blocks[span['block']][1]
                assert (span['offset'], span['bytes'], span['sha256']) == (offset, len(payload), hashlib.sha256(payload).hexdigest())
        reverse = run(name + '-reversed', selected[::-1])
        assert reverse['skin'] == deformed and reverse['pose_set']['objects'] == pose['objects'][::-1]
        assert (reverse['retained_bytes'], reverse['work_units']) == (value['retained_bytes'], value['work_units'])
        path = requests / (name + '-old-pose-set.json')
        path.write_text(json.dumps(dict(schema_version=1, expected_sha256=base['expected_source_sha256'], requests=selected)) + '\n')
        frozen[str(path)] = sha(path)
        args = ['nif-source-pose-set', str(source), '--request', str(path)]
        old, old_path = invoke(prior_binary, name + '-prior-pose-set', args)
        current, current_path = invoke(binary, name + '-current-pose-set', args)
        assert old_path.read_bytes() == current_path.read_bytes()
        assert old['evaluation']['objects'] == pose['objects'] and old['evaluation']['propagated_objects'] == 4
        equal.append(name); observations.append(dict(name=name, usage=dict(retained_bytes=value['retained_bytes'], work_units=value['work_units'], preparation=pose['preparation'])))

    signed = run('signed-zero', channels(-0., -0., -0.))
    assert signed['skin']['positions'] == CASES[1][3]
    assert signed['skin']['skin_to_source_world'] == CASES[1][5]
    selected = channels(0, 0, 0)
    stale = base['expected_source_sha256'].copy(); stale[0] ^= 1
    run('stale', selected, 'source SHA256 differs', patch=dict(expected_source_sha256=stale))
    run('geometry', selected, 'selected geometry has no decoded skin owner', patch=dict(geometry=0))
    for ordinal, label in enumerate(['child', 'root', 'parent']):
        run('missing-' + label, selected[:ordinal] + selected[ordinal+1:], 'not explicitly selected')
    run('duplicate', [selected[0], selected[0]], 'duplicate pose set object')
    wrong = copy.deepcopy(selected); wrong[0]['controller'] = 9
    run('wrong-link', wrong, 'object.controller differs')
    before = copy.deepcopy(selected); before[0]['source_time'] = -3
    run('before', before, 'extrapolate')
    late = copy.deepcopy(selected); late[0]['source_time'] = 3
    run('late', late[1:] + late[:1], 'extrapolate')
    run('negative-tolerance', selected, True, patch=dict(absolute_weight_tolerance=-1))
    mutants = []
    changed = fixture(); changed[8] = ('NiTransformController', writer.words(9) + changed[8][1][4:]); mutants.append(('chain', changed, (14,), selected))
    changed = fixture(); changed[5] = ('NiTransformData', writer.words(1, 1) + writer.floats(-2, 1, 0, 0, 0) + changed[5][1][4:]); mutants.append(('active-rotation', changed, (14,), selected))
    changed = fixture(); changed[14] = ('NiNode', writer.node(writer.RM90, [-5, 3, 9], 2, [])); mutants.append(('orphan', changed, (14,), selected))
    changed = fixture(); changed[2] = ('NiSkinData', skin(first_weight=-.25)); mutants.append(('negative-weight', changed, (14,), selected))
    changed = fixture(); changed[2] = ('NiSkinData', skin(scale=0)); mutants.append(('singular-skin', changed, (14,), selected))
    changed = fixture(); changed += [('NiNode', writer.node(writer.ID, [0, 0, 0], 1, [], 18)), ('NiTransformController', controller(17, 6))]
    mutants.append(('unrelated', changed, (14, 17), selected + [dict(object=17, controller=18, source_time=0)]))
    for name, changed, roots, requests_for_source in mutants:
        path = inputs / (name + '.nif'); path.write_bytes(container(changed, roots)); frozen[str(path)] = sha(path)
        run(name, requests_for_source, True, input_path=path)
    for path, digest in frozen.items(): assert sha(Path(path)) == digest, path
    result = dict(contract='engineering-complete-required-pose-set-skin-v1', literal_complete_cases=len(CASES),
                  request_permutations=len(CASES), signed_zero_case=True, intended_semantic_refusals=len(refusals),
                  prior_complete_pose_set_reports_byte_equal=equal, observations=observations, invocations=invocations,
                  frozen_hashes=frozen, original_process_used=False, retail_behavior_verified=False)
    (output / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--prior-binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = check(args.binary, args.prior_binary, args.output)
    print(json.dumps({k: v for k, v in result.items() if k not in ['invocations', 'frozen_hashes', 'observations']}))
