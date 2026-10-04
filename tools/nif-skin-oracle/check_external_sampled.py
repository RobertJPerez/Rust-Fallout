"""Independent three-source literal skin palette with shear and higher-ID rig.

Existing authored packet writers only; no copied sampler/deformer, inferred rig
alignment, controller clock or measured retail playback expectation.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_external as writer
import check_sampled as raw


def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()


def keys():
    return (raw.words(0, 2, 1) + raw.floats(-2, -3, 1, 2, 2, 5, -3, 6)
            + raw.words(2, 1) + raw.floats(-2, -1, 2, 3))


def clip_blocks():
    sequence = raw.words(raw.NULL, 1, 7, 1, raw.NULL) + bytes([25])
    sequence += raw.words(0, raw.NULL, 1, raw.NULL, raw.NULL) + raw.floats(.25)
    sequence += raw.words(raw.NULL, 3) + raw.floats(17, 100, 101)
    sequence += raw.words(raw.NULL, raw.NULL) + raw.shorts(2) + raw.words(raw.NULL, raw.NULL)
    return [('NiControllerSequence', sequence),
            ('NiTransformInterpolator', raw.floats(100, 200, 300, 2, -3, 4, -5, 12) + raw.words(2)),
            ('NiTransformData', keys())]


def rig_blocks():
    _, original = writer.fixtures()
    blocks = [original[3], original[0], original[4], original[2], original[1]]
    blocks[3] = ('NiNode', raw.words(0) + raw.node([[-1, 0, 0], [0, -1, 0], [0, 0, 1]], [3, -1, 2], 2, [0], 5)[4:])
    blocks[4] = ('NiNode', raw.node(raw.R90, [999, 888, 777], 99, [3, 2]))
    controller = raw.words(raw.NULL) + raw.shorts(0x004c) + raw.floats(17, -9, 100, 101) + raw.words(3, 6)
    blocks += [('NiTransformController', controller),
               ('NiTransformInterpolator', raw.floats(1000, 2000, 3000, 2, -3, 4, -5, 12) + raw.words(7)),
               ('NiTransformData', keys())]
    return blocks


CASES = [
    ('first-reflected', -2,
     [[[.5, .5, 0, -1.5], [0, -.5, 0, -1], [0, 0, -1, 3.5]],
      [[-.5, -.5, 0, .25], [0, .5, 0, -3], [0, 0, -1, 4.5]]],
     [[-.4375, -2.75, 1.25], [-.5, -3, 2.5], [-1.25, -1.5, 5.5]],
     [[-.75, .5, 0], [1.5, -1, 0], [-1.5, 1, 0]],
     [[0, -99, 0, 900], [99, 0, 0, 591], [0, 0, -99, 975]]),
    ('zero-forward', -1,
     [[[0, 0, 0, -.5], [0, 0, 0, -.5], [0, 0, 0, 6.5]]] * 2,
     [[-.5, -.5, 6.5]] * 3, [[0, 0, 0]] * 3,
     [[0, 0, 0, 999], [0, 0, 0, 789], [0, 0, 0, 1074]]),
    ('interior', 0,
     [[[-.5, -.5, 0, .5], [0, .5, 0, 0], [0, 0, 1, 9.5]],
      [[.5, .5, 0, -1.25], [0, -.5, 0, 2], [0, 0, 1, 8.5]]],
     [[-.5625, 1.75, 11.75], [-.5, 2, 10.5], [.25, .5, 7.5]],
     [[.75, -.5, 0], [-1.5, 1, 0], [1.5, -1, 0]],
     [[0, 99, 0, 1098], [-99, 0, 0, 987], [0, 0, 99, 1173]]),
    ('last', 2,
     [[[-1.5, -1.5, 0, 2.5], [0, 1.5, 0, 1], [0, 0, 3, 15.5]],
      [[1.5, 1.5, 0, -2.75], [0, -1.5, 0, 7], [0, 0, 3, 12.5]]],
     [[-.6875, 6.25, 22.25], [-.5, 7, 18.5], [1.75, 2.5, 9.5]],
     [[2.25, -1.5, 0], [-4.5, 3, 0], [4.5, -3, 0]],
     [[0, 297, 0, 1296], [-297, 0, 0, 1383], [0, 0, 297, 1371]]),
]


def check(binary, prior_binary, output):
    binary = binary.resolve(); prior_binary = prior_binary.resolve()
    output.mkdir(parents=True, exist_ok=False)
    skin_dir = output / 'skin-inputs'; skin_dir.mkdir()
    rig_dir = output / 'rig-inputs'; rig_dir.mkdir()
    clip_dir = output / 'clip-inputs'; clip_dir.mkdir()
    requests = output / 'requests'; requests.mkdir()
    skin = skin_dir / 'authored.nif'; rig = rig_dir / 'authored.nif'; clip = clip_dir / 'authored.kf'
    skin.write_bytes(writer.fixtures()[0]); rig.write_bytes(writer.container(rig_blocks(), [4], [b'Rig\0', b'Twin']))
    clip.write_bytes(writer.container(clip_blocks(), [0], [b'Rig\0', b'NiTransformController']))
    frozen = {str(p): sha(p) for p in [skin, rig, clip, binary, prior_binary]}
    base = dict(schema_version=1, expected_skin_sha256=list(bytes.fromhex(sha(skin))),
                expected_rig_sha256=list(bytes.fromhex(sha(rig))), expected_clip_sha256=list(bytes.fromhex(sha(clip))),
                geometry=3, rig_root=4, explicit_bone_mapping=[
                    dict(bone_ordinal=0, rig_node=3, expected_skin_bone_name_bytes=list(b'Skin\xff\0'), expected_rig_node_name_bytes=list(b'Rig\0')),
                    dict(bone_ordinal=1, rig_node=0, expected_skin_bone_name_bytes=list(b'Twin'), expected_rig_node_name_bytes=list(b'Twin'))],
                explicit_root_space_mapping=[[1, 1, 0, -2], [0, -1, 0, 3], [0, 0, 2, 1]],
                weights=dict(kind='require_unit_sum', absolute_tolerance=0), clip_object=3,
                clip_node_name_bytes=list(b'Rig\0'), clip_sequence=0, clip_controlled_ordinal=0, source_time=0)
    invocations = []; equal = []; cases = []; refusals = []

    def invoke(exe, name, args, code=0):
        destination = output / (name + '.json'); command = [str(exe), *args, '--output', str(destination)]
        process = subprocess.run(command, capture_output=True, text=True)
        (output / (name + '.stderr.txt')).write_text(process.stderr)
        assert process.returncode == code, (name, process.stderr)
        report = json.loads(destination.read_text()); assert report['failures'] == code, report
        invocations.append(dict(command=command, exit_code=process.returncode, receipt_sha256=sha(destination)))
        return report, destination

    def run(name, request, error=False, rig_input=rig, clip_input=clip):
        path = requests / (name + '.json'); path.write_text(json.dumps(request) + '\n'); frozen[str(path)] = sha(path)
        report, _ = invoke(binary, name, ['nif-external-clip-skin', str(skin), str(rig_input), str(clip_input), '--request', str(path)], int(error))
        if error:
            assert report['evaluation'] is None and report['error'], report
            refusals.append(name); return None
        value = report['evaluation']; assert (value['rig_scene_decodes'], value['skin_scene_decodes']) == (1, 1)
        assert value['sample']['skeleton_sha256'] == value['external']['rig_source_sha256'] == sha(rig_input)
        assert value['sample']['clip_sha256'] == sha(clip_input) and value['external']['skin_source_sha256'] == sha(skin)
        assert value['sample']['requested_time_f64_bits'] == struct.unpack('<Q', struct.pack('<d', request['source_time']))[0]
        assert value['sample']['unapplied_object_controller'] == 5 and value['external']['rig_unapplied_controllers'] == []
        assert not value['retail_behavior_verified'] and not value['sample']['retail_behavior_verified'] and not value['external']['retail_behavior_verified']
        return value

    for name, time, palette, positions, normals, world in CASES:
        request = dict(base, source_time=time); value = run(name, request); deformed = value['external']['skin']
        assert [p['node'] for p in deformed['palette']] == [3, 0]
        assert [p['matrix'] for p in deformed['palette']] == palette, (name, deformed['palette'])
        assert deformed['positions'] == positions, (name, deformed['positions'])
        assert deformed['normals'] == normals, (name, deformed['normals'])
        assert deformed['skin_to_source_world'] == [[0, -4, 0, -4], [4, 0, 0, -6], [0, 0, 4, -4]]
        assert deformed['weight_sums'] == [1, 1, 1] and value['sample']['source_world'] == world
        reverse = copy.deepcopy(request); reverse['explicit_bone_mapping'].reverse()
        assert run(name + '-reversed-map', reverse) == value
        for key, source, blocks in [('object', rig, rig_blocks()), ('sequence', clip, clip_blocks()), ('interpolator', clip, clip_blocks()), ('data', clip, clip_blocks())]:
            span = value['sample'][key]; payload = blocks[span['block']][1]
            offset = len(source.read_bytes()) - 8 - sum(len(p) for _, p in blocks[span['block']:])
            assert (span['offset'], span['bytes'], span['sha256']) == (offset, len(payload), hashlib.sha256(payload).hexdigest())
        assert value['sample']['controlled_packet']['interpolator'] == 1 and value['sample']['controlled_ordinal'] == 0
        pair_request = requests / (name + '-clip-only.json')
        pair_request.write_text(json.dumps(dict(schema_version=1, expected_skeleton_sha256=base['expected_rig_sha256'], expected_clip_sha256=base['expected_clip_sha256'],
            object=3, node_name_bytes=base['clip_node_name_bytes'], sequence=0, controlled_ordinal=0, source_time=time)) + '\n')
        frozen[str(pair_request)] = sha(pair_request); args = ['nif-clip-pose', str(rig), str(clip), '--request', str(pair_request)]
        old, old_path = invoke(prior_binary, name + '-prior-clip', args)
        current, current_path = invoke(binary, name + '-current-clip', args)
        assert old_path.read_bytes() == current_path.read_bytes() and old['evaluation'] == value['sample']
        equal.append(name + '-clip'); cases.append(dict(name=name, retained_bytes=value['retained_bytes'], work_units=value['work_units']))

    signed = run('signed-zero', dict(base, source_time=-0.))
    assert signed['external']['skin']['positions'] == CASES[2][3]
    stored = {k: v for k, v in base.items() if k not in ['expected_clip_sha256', 'clip_object', 'clip_node_name_bytes', 'clip_sequence', 'clip_controlled_ordinal', 'source_time']}
    path = requests / 'stored-external.json'; path.write_text(json.dumps(stored) + '\n'); frozen[str(path)] = sha(path)
    args = ['nif-skin', str(skin), '--external-rig', str(rig), '--external-skin-request', str(path)]
    old, old_path = invoke(prior_binary, 'prior-stored-external', args); current, current_path = invoke(binary, 'current-stored-external', args)
    assert old_path.read_bytes() == current_path.read_bytes(); equal.append('stored-external')
    for field in ['expected_skin_sha256', 'expected_rig_sha256', 'expected_clip_sha256']:
        wrong = copy.deepcopy(base); wrong[field][0] ^= 1; run('stale-' + field, wrong, True)
    for field, bad in [('clip_object', 0), ('clip_node_name_bytes', list(b'Rig')), ('clip_sequence', 1),
                       ('clip_controlled_ordinal', 1), ('source_time', -3), ('source_time', 3), ('rig_root', 0)]:
        run('wrong-' + field + '-' + str(bad).replace(' ', ''), dict(base, **{field: bad}), True)
    wrong = copy.deepcopy(base); wrong['explicit_bone_mapping'][1]['bone_ordinal'] = 0; run('duplicate-ordinal', wrong, True)
    wrong = copy.deepcopy(base); wrong['explicit_bone_mapping'][1]['rig_node'] = 3; run('duplicate-target', wrong, True)
    wrong = copy.deepcopy(base); wrong['explicit_bone_mapping'].pop(); run('incomplete-map', wrong, True)
    wrong = copy.deepcopy(base); wrong['explicit_bone_mapping'][1]['expected_skin_bone_name_bytes'].append(0); run('raw-skin-name', wrong, True)
    witness = [[478384076, 548195520, 675781114, 0], [95565559, 470460823, 587107439, 0], [573949635, 1018656343, 1262888553, 0]]
    run('corrected-singular-mapping', dict(base, explicit_root_space_mapping=witness), True)
    for name, node, word in [('other-required-controller', 0, 8), ('controlled-ancestor', 4, 8), ('ambiguous-name', 2, 0)]:
        changed = rig_blocks(); payload = bytearray(changed[node][1]); payload[word:word+4] = raw.words(0 if word == 0 else 5); changed[node] = (changed[node][0], bytes(payload))
        path = rig_dir / (name + '.nif'); path.write_bytes(writer.container(changed, [4], [b'Rig\0', b'Twin'])); frozen[str(path)] = sha(path)
        run(name, dict(base, expected_rig_sha256=list(bytes.fromhex(sha(path)))), True, rig_input=path)
    changed = clip_blocks(); changed[2] = ('NiTransformData', raw.words(1, 1) + raw.floats(-2, 1, 0, 0, 0) + changed[2][1][4:])
    path = clip_dir / 'active-rotation.kf'; path.write_bytes(writer.container(changed, [0], [b'Rig\0', b'NiTransformController'])); frozen[str(path)] = sha(path)
    run('active-rotation', dict(base, expected_clip_sha256=list(bytes.fromhex(sha(path)))), True, clip_input=path)
    changed = rig_blocks(); payload = bytearray(changed[3][1]); payload[8:12] = raw.words(raw.NULL); changed[3] = ('NiNode', bytes(payload))
    changed[2] = ('NiNode', raw.words(2) + changed[2][1][4:]); outside = rig_dir / 'outside.nif'
    outside.write_bytes(writer.container(changed, [4], [b'Rig\0', b'Twin', b'Unrelated'])); frozen[str(outside)] = sha(outside)
    unrelated = clip_dir / 'outside.kf'; unrelated.write_bytes(writer.container(clip_blocks(), [0], [b'Unrelated', b'NiTransformController'])); frozen[str(unrelated)] = sha(unrelated)
    run('outside-mapped-path', dict(base, expected_rig_sha256=list(bytes.fromhex(sha(outside))), expected_clip_sha256=list(bytes.fromhex(sha(unrelated))),
        clip_object=2, clip_node_name_bytes=list(b'Unrelated')), True, rig_input=outside, clip_input=unrelated)
    for path, digest in frozen.items(): assert sha(Path(path)) == digest, path
    result = dict(contract='engineering-one-exact-external-clip-skin-v1', literal_complete_cases=len(CASES), mapping_permutations=len(CASES),
                  signed_zero_case=True, intended_semantic_refusals=len(refusals), prior_complete_reports_byte_equal=equal, cases=cases,
                  invocations=invocations, frozen_hashes=frozen, original_process_used=False, retail_behavior_verified=False)
    (output / 'summary.json').write_text(json.dumps(result, indent=2) + '\n'); return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--prior-binary', type=Path, required=True); parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(); result = check(args.binary, args.prior_binary, args.output)
    print(json.dumps({k: v for k, v in result.items() if k not in ['invocations', 'frozen_hashes', 'cases']}))
