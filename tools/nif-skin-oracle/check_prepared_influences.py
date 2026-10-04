"""Literal sparse weights through sealed source/CSR reuse and the actual CLI.

Reuse only authored byte writers. No decoder/deformation algorithm or original
playback expectation enters this check. Each run requires a fresh output tree.
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


def fixture():
    blocks = writer.fixture()
    skin = writer.transform(writer.ID, [1, -2, 3], .5) + writer.words(2) + bytes([1])
    for rotation, translation, scale, influences in [
        (writer.ID, [-1, 0, 2], 1, [(2, .125), (0, .375), (0, .25), (1, 1),
                                  (0, -0.), (0, .125), (0, .125), (0, 0)]),
        (writer.R90, [0, -1, 0], 2, [(0, .5), (2, .875)]),
    ]:
        skin += writer.transform(rotation, translation, scale) + writer.floats(0, 0, 0, 10)
        skin += writer.shorts(len(influences))
        for vertex, weight in influences:
            skin += writer.shorts(vertex) + writer.floats(weight)
    blocks[5] = ('NiSkinData', skin)
    return blocks


def check(binary, prior_binary, output):
    binary = binary.resolve(); prior_binary = prior_binary.resolve(); output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = output / 'inputs'; inputs.mkdir()
    requests = output / 'requests'; requests.mkdir()
    source = inputs / 'authored.nif'; source.write_bytes(writer.container(fixture()))
    assert sha(source) == 'e929d50a3ba1a1a6762729a5648c768c9a5119b81df2f33a5911d11349aaefd6'
    raw = dict(kind='preserve_raw_nonnegative')
    unit = dict(kind='require_unit_sum', absolute_tolerance=.375)
    base = dict(schema_version=1, expected_sha256=list(bytes.fromhex(sha(source))),
                geometry=3, weight_policies=[raw.copy(), unit.copy(), raw.copy()])
    frozen = {str(path): sha(path) for path in [source, binary, prior_binary]}
    commands = []; refusals = []; equal = []

    def request(name, value):
        path = requests / (name + '.json')
        path.write_text(json.dumps(value) + '\n'); frozen[str(path)] = sha(path)
        return path

    def invoke(exe, name, args, error=None, destination=None):
        dest = destination or output / (name + '.json')
        command = [str(exe), *args, '--output', str(dest)]
        process = subprocess.run(command, capture_output=True, text=True)
        (output / (name + '.stderr.txt')).write_text(process.stderr)
        assert process.returncode == (2 if error == 'conflict' else int(error is not None)), (name, process.stderr)
        value = None
        if error in ('schema', 'count', 'protected', 'conflict'):
            assert not dest.exists(), name
            needles = dict(schema=('unknown field', 'missing field', 'invalid type', 'expected value', 'unknown variant'),
                           count=('requires schema1', 'requires 1..64'),
                           protected=('outside every source directory',),
                           conflict=('cannot be used with',))[error]
            assert any(text in process.stderr for text in needles), (name, process.stderr)
        else:
            value = json.loads(dest.read_text())
            assert value['failures'] == int(error is not None), value
            if error:
                assert value['evaluation'] is None and error in value['error'], value
            else:
                assert value['evaluation'] is not None and value['sha256'] == sha(source), value
        commands.append(dict(command=command, exit_code=process.returncode, intended_error=error,
                             receipt_sha256=sha(dest) if dest.exists() else None))
        return value, dest

    def run(name, value, error=None, destination=None):
        report, path = invoke(binary, name,
            ['nif-skin', str(source), '--prepared-influences-request', str(request(name, value))],
            error, destination)
        if error:
            refusals.append(name); return None
        frozen[str(path)] = sha(path)
        return report['evaluation']

    first = run('ordered', base)
    assert first['preparation']['binding_decodes'] == first['preparation']['scene_decodes'] == 1
    assert first['reuse'] == dict(initial_binding_decodes=2, initial_scene_decodes=2,
                                 initial_source_hash_byte_visits=source.stat().st_size * 2,
                                 initial_csr_constructions=1,
                                 binding_decodes_per_evaluation=0, scene_decodes_per_evaluation=0,
                                 source_hash_byte_visits_per_evaluation=0, csr_constructions_per_evaluation=0)
    table = first['table']
    assert table['vertex_offsets'] == [0, 7, 8, 10]
    assert [(entry['bone_ordinal'], entry['source_weight_ordinal'], entry['weight_bits']) for entry in table['entries']] == [
        (0, 1, 0x3ec00000), (0, 2, 0x3e800000), (0, 4, 0x80000000), (0, 5, 0x3e000000),
        (0, 6, 0x3e000000), (0, 7, 0), (1, 0, 0x3f000000), (0, 3, 0x3f800000),
        (0, 0, 0x3e000000), (1, 1, 0x3f600000)]
    for pose in first['poses']:
        assert pose['positions'] == [[2.8125, .0625, 13.25], [6.5, 1.5, 6], [-7.1875, -1.375, .9375]]
        assert pose['normals'] == [[-1.25, .625, 0], [2, -1, 0], [-5, 2.5, 0]]
        assert pose['weight_sums'] == [1.375, 1, 1]
        assert [p['matrix'] for p in pose['palette']] == [
            [[0, 1, 0, 2.5], [-1, 0, 0, -.5], [0, 0, 1, 5]],
            [[0, -3, 0, 0], [3, 0, 0, -1.5], [0, 0, 3, 3.5]]]
        assert pose['skin_to_source_world'] == [[0, -4, 0, -4], [4, 0, 0, -6], [0, 0, 4, -4]]
        assert pose['unapplied_controllers'] == [dict(object=1, controller=7)]
        assert pose['retained_bytes'] > (first['preparation']['retained_bytes']
            + first['preparation']['source_binding_retained_bytes'] + table['usage']['output_bytes'])
        assert not pose['retail_behavior_verified']
    assert not first['retail_behavior_verified'] and not table['retail_behavior_verified']
    assert first['work_units'] == first['preparation']['work_units'] + table['usage']['work_units'] + source.stat().st_size + sum(p['work_units'] for p in first['poses'])
    for ordinal, policy in enumerate(base['weight_policies']):
        old_request = request('cold-' + str(ordinal), dict(schema_version=1,
            expected_sha256=base['expected_sha256'], geometry=3, weights=policy))
        args = ['nif-skin', str(source), '--influences-request', str(old_request)]
        cold, cold_path = invoke(binary, 'cold-' + str(ordinal), args)
        prior, prior_path = invoke(prior_binary, 'prior-' + str(ordinal), args)
        assert cold_path.read_bytes() == prior_path.read_bytes(), ordinal
        assert table == cold['evaluation']['table']
        observed = copy.deepcopy(first['poses'][ordinal]); expected = cold['evaluation']['pose']
        for key in ['retained_bytes', 'work_units']:
            observed.pop(key); expected.pop(key)
        assert observed == expected, ordinal
        equal.append(dict(policy=policy, complete_old_report_sha256=sha(cold_path)))
    permutation = [1, 0, 2]
    permuted = run('permuted', dict(base, weight_policies=[base['weight_policies'][i] for i in permutation]))
    assert permuted == dict(first, poses=[first['poses'][i] for i in permutation])
    maximum = run('maximum', dict(base, weight_policies=[raw.copy() for _ in range(64)]))
    assert maximum['table'] == table and len(maximum['poses']) == 64
    assert maximum['poses'] == [first['poses'][0]] * 64
    for name, value, error in [
        ('stale', dict(base, expected_sha256=[base['expected_sha256'][0] ^ 1, *base['expected_sha256'][1:]]), 'prepared table source SHA256 differs'),
        ('geometry', dict(base, geometry=1), 'selected geometry has no decoded skin owner'),
        ('unit-late', dict(base, weight_policies=[raw, dict(kind='require_unit_sum', absolute_tolerance=0)]), 'raw weight sum 1.375'),
        ('bad-tolerance', dict(base, weight_policies=[dict(kind='require_unit_sum', absolute_tolerance=-1)]), 'tolerance must be finite'),
        ('schema-version', dict(base, schema_version=2), 'count'),
        ('empty', dict(base, weight_policies=[]), 'count'),
        ('too-many', dict(base, weight_policies=[raw] * 65), 'count'),
        ('top-extra', dict(base, extra=1), 'schema'),
        ('raw-extra', dict(base, weight_policies=[dict(raw, extra=1)]), 'schema'),
        ('raw-tolerance', dict(base, weight_policies=[dict(raw, absolute_tolerance=0)]), 'schema'),
        ('unit-extra', dict(base, weight_policies=[dict(unit, extra=1)]), 'schema'),
        ('unit-missing-tolerance', dict(base, weight_policies=[dict(kind='require_unit_sum')]), 'schema'),
        ('unknown-policy', dict(base, weight_policies=[dict(kind='normalize')]), 'schema'),
        ('missing-kind', dict(base, weight_policies=[{}]), 'schema'),
        ('wrong-tolerance-type', dict(base, weight_policies=[dict(kind='require_unit_sum', absolute_tolerance='0')]), 'schema'),
        ('nonfinite-tolerance', dict(base, weight_policies=[dict(kind='require_unit_sum', absolute_tolerance=float('nan'))]), 'schema'),
    ]:
        run(name, value, error)
    for missing in ['expected_sha256', 'geometry', 'weight_policies']:
        invalid = copy.deepcopy(base); invalid.pop(missing); run('missing-' + missing, invalid, 'schema')
    run('protected-source', base, 'protected', inputs / 'blocked.json')
    run('protected-request', base, 'protected', requests / 'blocked.json')
    flag_request = request('conflicts', base)
    for ordinal, flags in enumerate([
        ['--pose-geometry', '3', '--pose-weight-tolerance', '0'], ['--oracle-report', str(flag_request)],
        ['--include-partitions'], ['--include-bindings'], ['--sampled-pose-request', str(flag_request)],
        ['--influences-request', str(flag_request)], ['--external-rig', str(source), '--external-skin-request', str(flag_request)],
        ['--shared-skin-request', str(flag_request)], ['--partition-streams-request', str(flag_request)],
        ['--partition-pose-request', str(flag_request)], ['--pose-set-request', str(flag_request)], ['--bounds-request', str(flag_request)],
    ]):
        invoke(binary, 'conflict-' + str(ordinal), ['nif-skin', str(source), '--prepared-influences-request', str(flag_request), *flags], 'conflict')
        refusals.append('conflict-' + str(ordinal))
    assert run('after-refusals', base) == first
    for path, digest in frozen.items():
        assert sha(Path(path)) == digest, path
    summary = dict(contract='engineering-prepared-source-influence-skin-v1',
        literal_evaluations=3, maximum_reused_evaluations=64, old_complete_reports=equal,
        intended_refusals=refusals, source_sha256=sha(source), commands=commands,
        frozen_hashes=frozen, retail_behavior_verified=False)
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
