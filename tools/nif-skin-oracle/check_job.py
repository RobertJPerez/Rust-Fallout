"""Three authored geometries, literal CPU math and cooperative CLI lifecycle.

Only byte writers are shared. No parser, scheduler or deformation algorithm is
copied into these literal expectations. Original data and binaries stay local.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import check_sampled as writer


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fixture(nonunit_last=False):
    blocks = writer.fixture()
    blocks[0] = ('NiNode', writer.node(writer.R90, [4, -2, 8], 2, [1, 2, 3, 10, 14]))
    second = writer.av(writer.RM90, [1000, -2000, 3000], 77) + writer.words(13, 11, 0, writer.NULL) + bytes([0])
    skin = writer.transform(writer.R90, [-3, 2, -1], 1) + writer.words(2) + bytes([1])
    for _ in range(2):
        skin += writer.transform(writer.ID, [0, 0, 0], 1) + writer.floats(0, 0, 0, 10) + writer.shorts(3)
        for vertex in range(3):
            skin += writer.shorts(vertex) + writer.floats(.5)
    blocks.extend([('NiTriShape', second), ('NiSkinInstance', writer.words(12, writer.NULL, 0, 2, 2, 1)),
                   ('NiSkinData', skin), ('NiTriShapeData', blocks[6][1])])
    third = writer.av(writer.ID, [-100, 200, -300], -99) + writer.words(17, 15, 0, writer.NULL) + bytes([0])
    skin = writer.transform(writer.ID, [0, 0, 0], 1) + writer.words(1) + bytes([1])
    skin += writer.transform(writer.ID, [0, 0, 0], 1) + writer.floats(0, 0, 0, 10) + writer.shorts(3)
    for vertex in range(3):
        skin += writer.shorts(vertex) + writer.floats(.5 if nonunit_last else 1)
    blocks.extend([('NiTriShape', third), ('NiSkinInstance', writer.words(16, writer.NULL, 0, 1, 2)),
                   ('NiSkinData', skin), ('NiTriShapeData', blocks[6][1])])
    return writer.container(blocks)


def check(binary, prior_binary, output):
    binary = binary.resolve(); prior_binary = prior_binary.resolve(); output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = output / 'inputs'; inputs.mkdir()
    requests = output / 'requests'; requests.mkdir()
    source = inputs / 'three-geometries.nif'; source.write_bytes(fixture())
    nonunit = inputs / 'last-nonunit.nif'; nonunit.write_bytes(fixture(True))
    base = dict(schema_version=1, expected_source_sha256=list(bytes.fromhex(sha(source))),
        geometries=[dict(geometry=geometry, weights=dict(kind='require_unit_sum', absolute_tolerance=0))
                    for geometry in [3, 10, 14]], step_geometry_caps=[1, 1, 1], cancel_after_completed=None)
    frozen = {str(path): sha(path) for path in [binary, prior_binary, source, nonunit]}
    commands = []; refusals = []; completed = []

    def request(name, value):
        path = requests / (name + '.json'); path.write_text(json.dumps(value) + '\n')
        frozen[str(path)] = sha(path); return path

    def invoke(exe, name, args, error=None, destination=None):
        dest = destination or output / (name + '.json')
        command = [str(exe), *args, '--output', str(dest)]
        process = subprocess.run(command, capture_output=True, text=True)
        (output / (name + '.stderr.txt')).write_text(process.stderr)
        assert process.returncode == (2 if error == 'conflict' else int(error is not None)), (name, process.stderr)
        value = None
        if error in ('schema', 'count', 'protected', 'conflict'):
            assert not dest.exists(), name
            needles = dict(schema=('unknown field', 'missing field', 'unknown variant', 'invalid type', 'invalid value', 'expected value'),
                count=('requires schema1', 'requires 1..128', 'cancellation boundary exceeds'),
                protected=('outside every source directory',), conflict=('cannot be used with',))[error]
            assert any(text in process.stderr for text in needles), (name, process.stderr)
        else:
            value = json.loads(dest.read_text()); assert value['failures'] == int(error is not None), value
            if error:
                assert value['evaluation'] is None and error in value['error'], value
            else:
                assert value['evaluation'] is not None
        commands.append(dict(command=command, exit_code=process.returncode, intended_error=error,
                             receipt_sha256=sha(dest) if dest.exists() else None))
        return value, dest

    def run(name, value, error=None, input_path=source, destination=None):
        report, dest = invoke(binary, name, ['nif-skin', str(input_path), '--shared-skin-job-request', str(request(name, value))], error, destination)
        if report is not None:
            assert report['sha256'] == sha(input_path)
            assert report['retained_bytes'] <= 128 * 1024 * 1024 and report['work_units'] <= 128_000_000
            for poll in report['progress']:
                assert not any(isinstance(item, (list, dict)) for item in poll.values()), poll
            if report['admission'] is not None:
                a = report['admission']; p = report['progress'][-1]
                assert report['retained_bytes'] == report['driver_retained_bytes'] + a['retained_bytes'] + p['evaluation_retained_bytes']
                assert report['work_units'] == a['work_units'] + p['evaluation_work_units']
        if error:
            refusals.append(name)
        else:
            frozen[str(dest)] = sha(dest); completed.append(name)
        return report

    first = run('one-at-a-time', base)
    batch = first['evaluation']; geometries = batch['geometries']
    assert [p['geometry'] for p in geometries] == [3, 10, 14]
    assert [p['positions'] for p in geometries] == [
        [[2.625, 2.75, 11.375], [6.5, 1.5, 6], [-9, -1.5, .5]],
        [[-2, 4.5, 7], [-13.5, 3.5, 2], [-10, 5.5, -3]],
        [[4, 1, 10], [-8, 16, 4], [-2, 13, -2]]]
    assert [p['normals'] for p in geometries] == [
        [[-4, 2, 0], [2, -1, 0], [-6, 3, 0]], [[-2, 3.5, 0]] * 3, [[3, 6, 0]] * 3]
    assert [p['skin_to_source_world'] for p in geometries] == [
        [[0, -4, 0, -4], [4, 0, 0, -6], [0, 0, 4, -4]],
        [[2, 0, 0, 10], [0, 2, 0, -6], [0, 0, 2, 10]],
        [[0, -2, 0, 4], [2, 0, 0, -2], [0, 0, 2, 8]]]
    assert geometries[2]['palette'] == [dict(ordinal=0, node=2, matrix=[[3, 0, 0, -2], [0, 3, 0, 4], [0, 0, 3, 1]])]
    assert all(p['weight_sums'] == [1, 1, 1] and not p['retail_behavior_verified'] for p in geometries)
    assert first['admission']['source_preparation']['binding_decodes'] == first['admission']['source_preparation']['scene_decodes'] == 1
    for name, caps in [('two-at-a-time', [2, 2]), ('all', [64]), ('zero-yields', [0, 1, 0, 2])]:
        value = run(name, dict(base, step_geometry_caps=caps))
        assert value['evaluation'] == batch, name
        polls = value['progress'][1:]
        for cap, poll in zip(caps, polls):
            assert poll['advanced_geometries'] <= cap
            if cap == 0:
                assert poll['advanced_geometries'] == poll['step_work_units'] == poll['step_retained_bytes'] == 0
        assert sum(p['step_work_units'] for p in polls) == sum(g['work_units'] for g in geometries)
        assert sum(p['step_retained_bytes'] for p in polls) == sum(g['retained_bytes'] for g in geometries)
    old_request = request('old-shared', {key: base[key] for key in ['schema_version', 'expected_source_sha256', 'geometries']})
    args = ['nif-skin', str(source), '--shared-skin-request', str(old_request)]
    old, old_path = invoke(prior_binary, 'prior-shared', args)
    current, current_path = invoke(binary, 'current-shared', args)
    assert current_path.read_bytes() == old_path.read_bytes()
    assert old['evaluation']['geometries'] == geometries
    for key in ['source_sha256', 'preparation', 'decoder_array_admission_bytes', 'decoder_check_admission_units', 'source_binding_retained_bytes']:
        assert batch[key] == old['evaluation'][key], key
    assert batch['retained_bytes'] == first['admission']['retained_bytes'] + sum(p['retained_bytes'] for p in geometries)
    assert batch['work_units'] == first['admission']['work_units'] + sum(p['work_units'] for p in geometries)
    for boundary in [0, 1, 3]:
        report = run('cancel-' + str(boundary), dict(base, step_geometry_caps=[64], cancel_after_completed=boundary), 'skin job cancelled')
        assert report['progress'][-1]['state'] == 'cancelled' and report['progress'][-1]['completed_geometries'] == boundary
    run('incomplete', dict(base, step_geometry_caps=[0, 1]), 'no complete result')
    late = dict(base, expected_source_sha256=list(bytes.fromhex(sha(nonunit))), step_geometry_caps=[64])
    failed = run('later-failed', late, 'raw weight sum 0.5', nonunit)
    assert failed['progress'][-1]['state'] == 'failed' and failed['progress'][-1]['completed_geometries'] == 2
    assert failed['progress'][-1]['step_work_units'] > sum(p['work_units'] for p in geometries[:2])
    for name, value, error in [
        ('stale', dict(base, expected_source_sha256=[0] * 32), 'skin job source SHA256 differs'),
        ('duplicate', dict(base, geometries=[base['geometries'][0]] * 2), 'duplicate geometry'),
        ('empty', dict(base, geometries=[]), 'nonempty'),
        ('unknown-geometry', dict(base, geometries=[dict(geometry=999, weights=dict(kind='preserve_raw_nonnegative'))]), 'no decoded skin owner'),
        ('empty-steps', dict(base, step_geometry_caps=[]), 'count'),
        ('large-step', dict(base, step_geometry_caps=[65]), 'count'),
        ('many-steps', dict(base, step_geometry_caps=[0] * 129), 'count'),
        ('cancel-past-end', dict(base, cancel_after_completed=4), 'count'),
        ('schema-version', dict(base, schema_version=2), 'count'),
        ('many-geometries', dict(base, geometries=base['geometries'] * 22), 'count'),
        ('top-extra', dict(base, extra=1), 'schema'),
        ('negative-step', dict(base, step_geometry_caps=[-1]), 'schema'),
        ('raw-extra', dict(base, geometries=[dict(geometry=3, weights=dict(kind='preserve_raw_nonnegative', extra=1))]), 'schema'),
        ('raw-tolerance', dict(base, geometries=[dict(geometry=3, weights=dict(kind='preserve_raw_nonnegative', absolute_tolerance=0))]), 'schema'),
        ('unit-missing-tolerance', dict(base, geometries=[dict(geometry=3, weights=dict(kind='require_unit_sum'))]), 'schema'),
    ]:
        run(name, value, error)
    for name, folder in [('protected-source', inputs), ('protected-request', requests)]:
        run(name, base, 'protected', destination=folder / 'blocked.json')
    conflict_request = request('conflict', base)
    invoke(binary, 'conflict', ['nif-skin', str(source), '--shared-skin-job-request', str(conflict_request), '--shared-skin-request', str(old_request)], 'conflict')
    refusals.append('conflict')
    assert run('after-refusals', base)['evaluation'] == batch
    for path, digest in frozen.items():
        assert sha(Path(path)) == digest, path
    summary = dict(contract='engineering-cooperative-skin-job-v1', literal_geometries=3,
        successful_schedules=completed, intended_refusals=refusals,
        complete_prior_shared_report_byte_equal=True, source_sha256=sha(source),
        commands=commands, frozen_hashes=frozen, retail_behavior_verified=False)
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    return summary


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--prior-binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    result = check(arguments.binary, arguments.prior_binary, arguments.output)
    print(json.dumps({key: value for key, value in result.items() if key not in ('commands', 'frozen_hashes')}))
