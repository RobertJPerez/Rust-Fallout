"""Independent authored held-Boolean consumer; no retail clock or event claim.

Writes small source packets, then checks literal expectations from a frozen
production CLI. It does not parse NIF or share producer sampling logic.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

NULL = 2**32 - 1
CONTRACT = 'engineering-linked-local-visibility-v1'


def words(*values):
    return struct.pack('<' + 'I' * len(values), *values)


def floats(*values):
    return struct.pack('<' + 'f' * len(values), *values)


def fixture(target=0, chain=NULL, flags=0x4C, raw=2, data=3,
            times=(-2, 1, 8), values=(1, 0, 1), timeline=False):
    node = (words(NULL, 0, 1, 0xA9) + floats(5, -6, 7, 1, 0, 0, 0, 1, 0, 0, 0, 1, 2)
            + words(0, NULL, 0, 0))
    controller = words(chain) + struct.pack('<H', flags) + floats(13, -7, 100, 101) + words(target, 2)
    keys = words(len(times)) + (words(5) if times else b'')
    for time, value in zip(times, values):
        keys += floats(time) + bytes([value])
    packets = [node, controller, bytes([raw]) + words(data), keys]
    names = ['NiNode', 'NiVisController', 'NiBoolTimelineInterpolator' if timeline else 'NiBoolInterpolator', 'NiBoolData']
    out = (b'Gamebryo File Format, Version 20.2.0.7\n' + words(0x14020007)
           + b'\x01' + words(11, 4, 34) + b'\x00' * 3 + struct.pack('<H', 4))
    for name in names:
        out += words(len(name)) + name.encode('ascii')
    return (out + struct.pack('<4H', 0, 1, 2, 3) + words(*(len(packet) for packet in packets))
            + words(0, 0, 0) + b''.join(packets) + words(1, 0))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    args = parser.parse_args()
    args.output_dir.mkdir(exist_ok=False)
    inputs = args.output_dir / 'inputs'
    inputs.mkdir()
    binary = args.binary.resolve()
    before = digest(binary)
    source = inputs / 'authored.nif'
    source.write_bytes(fixture())
    invocations = []

    def invoke(name, path, time, visible=None, index=None, controller=1, error=None):
        receipt = args.output_dir / (name + '.json')
        command = [str(binary), 'nif-source-pose', str(path.resolve()), '--object', '0',
                   '--controller', str(controller), '--source-time', str(time), '--local-visibility',
                   '--output', str(receipt.resolve())]
        run = subprocess.run(command, capture_output=True, text=True)
        (args.output_dir / (name + '.stderr.txt')).write_text(run.stderr)
        if run.returncode != (1 if error else 0):
            raise ValueError(f'{name}: unexpected exit {run.returncode}: {run.stderr}')
        report = json.loads(receipt.read_text())
        if report['sha256'] != digest(path) or report['contract'] != CONTRACT:
            raise ValueError(f'{name}: source identity or contract differs')
        result = report['evaluation']
        if error:
            if report['failures'] != 1 or result is not None or error not in report['error']:
                raise ValueError(f'{name}: intended refusal missing')
        else:
            if (report['failures'] != 0 or result['retail_behavior_verified'] is not False
                or result['source_sha256'] != digest(path) or type(result['local_visible']) is not bool
                or result['local_visible'] is not visible or result['raw_value'] != int(visible)):
                raise ValueError(f'{name}: independent Boolean expectation differs')
            expected = {'kind': 'authored_pose'} if index is None else {
                'kind': 'held_key', 'index': index,
                'time_bits': struct.unpack('<I', floats((-2, 1, 8)[index]))[0]}
            if result['selection'] != expected:
                raise ValueError(f'{name}: literal key identity differs')
            if (result['unapplied_controller_fields']['frequency_bits'] != struct.unpack('<I', floats(13))[0]
                or result['unapplied_controller_fields']['start_bits'] != struct.unpack('<I', floats(100))[0]
                or result['object_flags'] != 0xA9):
                raise ValueError(f'{name}: source clocks or object flags changed')
        invocations.append(dict(command=command, exit_code=run.returncode,
                                receipt_sha256=digest(receipt), expected_error=error))

    # Literal authored states differ from Rust fixtures; caller binary64 times
    # straddle the middle binary32 key without rounding onto it.
    cases = [('first', -2, True, 0), ('held-first', 0, True, 0),
             ('before-middle', 1 - 2**-40, True, 0), ('middle', 1, False, 1),
             ('after-middle', 1 + 2**-40, False, 1), ('held-middle', 7, False, 1),
             ('last', 8, True, 2)]
    for name, time, visible, index in cases:
        invoke(name, source, time, visible, index)
    for raw in (0, 1):
        path = inputs / f'pose-{raw}.nif'
        path.write_bytes(fixture(raw=raw, data=NULL))
        invoke(f'pose-{raw}', path, 1.7976931348623157e308, bool(raw))
    invoke('nonfinite', source, 'NaN', error='must be finite')
    invoke('before-range', source, -3, error='extrapolate')
    invoke('after-range', source, 9, error='extrapolate')
    invoke('wrong-controller', source, 0, controller=2, error='object.controller differs')
    refusals = [
        ('wrong-target', fixture(target=1), 'controller.target differs'),
        ('chain', fixture(chain=1), 'controller chain'),
        ('manager', fixture(flags=0x6C), 'flags require unavailable semantics'),
        ('reserved-flags', fixture(flags=0x14C), 'flags require unavailable semantics'),
        ('undefined-cycle', fixture(flags=0x4E), 'flags require unavailable semantics'),
        ('timeline', fixture(timeline=True), 'timeline key crossing'),
        ('duplicate', fixture(times=(-2, -2, 8)), 'strictly increasing'),
        ('decreasing', fixture(times=(-2, 8, 1)), 'strictly increasing'),
        ('invalid-key', fixture(values=(1, 2, 1)), 'key Boolean value unavailable'),
        ('missing-pose', fixture(raw=2, data=NULL), 'pose Boolean value unavailable'),
        ('empty-data', fixture(raw=1, times=(), values=()), 'constant key group unavailable'),
    ]
    for name, payload, error in refusals:
        path = inputs / (name + '.nif')
        path.write_bytes(payload)
        invoke(name, path, 0, error=error)
    if digest(binary) != before or source.read_bytes() != fixture():
        raise ValueError('frozen binary or authored source changed')
    summary = dict(contract=CONTRACT, binary_sha256=before, source_sha256=digest(source),
                   analytic_cases=len(cases) + 2, intended_refusals=4 + len(refusals),
                   invocations=invocations, retail_behavior_verified=False)
    (args.output_dir / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
