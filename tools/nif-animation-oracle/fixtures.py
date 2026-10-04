"""Independent authored animation source fixture bytes.
Format facts come from pinned metadata; no source parser code is copied.
"""
from pathlib import Path
import struct, json, hashlib
N = 4294967295
STREAMS = (14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34)

def w(*v):
    return struct.pack('<' + 'I' * len(v), *v)

def h(*v):
    return struct.pack('<' + 'H' * len(v), *v)

def f(*v):
    return struct.pack('<' + 'f' * len(v), *v)

def container(stream, blocks, strings):
    types = list(dict.fromkeys((k for k, p in blocks)))
    out = b'Gamebryo File Format, Version 20.2.0.7\n' + w(335675399) + bytes([1]) + w(11, len(blocks), stream) + bytes(3) + h(len(types))
    out += b''.join((w(len(k)) + k.encode() for k in types)) + b''.join((h(types.index(k)) for k, p in blocks)) + b''.join((w(len(p)) for k, p in blocks))
    out += w(len(strings), max(map(len, strings), default=0)) + b''.join((w(len(s)) + s for s in strings)) + w(0)
    return out + b''.join((p for k, p in blocks)) + w(1, 1)

def fixture(stream, variant):
    strings = [b'clip', b'Bip01', b'Property', b'NiTransformController', b'controller\nid', b'interp', b'accum', b'\x80\xff', b'a\x00b\x00\x00', b'\r\n"\\']
    null = variant == 'null_links'
    unknown = variant == 'unknown_links'
    target = 7 if unknown else 6
    interp = 7 if unknown else 2
    controller = w(N if null else 7 if unknown else N) + h(65535) + w(2147483648, 2139095039, 4286578687, 1) + w(N if null else target, N if null else interp)
    transform = w(2147483648, 2139095039, 4286578687) + f(2, -3, 4, -5) + w(1, N if null else 7 if unknown else 5)
    keys = [] if variant == 'empty_arrays' else [(2.0, 8), (-0.0, 9), (2.0, 7)]
    text = w(0, len(keys)) + b''.join((f(t) + w(s) for t, s in keys))
    packets = [] if variant == 'empty_arrays' else [(N if null else interp, N if null else 7 if unknown else 0, 255, 1, 2, 3, 4, 5), (N if null else interp, N if null else 7 if unknown else 0, 3, 1, N, 3, 4, 5)]
    seq = w(0, len(packets), 2271560481)
    seq += b''.join((w(i, c) + bytes([p]) + w(n, pr, ct, ci, ii) for i, c, p, n, pr, ct, ci, ii in packets))
    cycle = 4294967295 if variant == 'unknown_cycle' else 2
    seq += f(-0.25) + w(N if null else 7 if unknown else 3, cycle) + f(1.25, -0.5, 4.0) + w(N if null else 7 if unknown else N, 6)
    if 24 <= stream <= 28:
        seq += w(N if null or variant == 'empty_arrays' else 7 if unknown else 4)
    elif stream > 28:
        refs = [] if variant == 'empty_arrays' else [N] if null else [7] if unknown else [4, N, 4]
        seq += h(len(refs)) + w(*refs)
    if variant == 'all_strings_null':
        text = w(N, len(keys)) + b''.join((f(t) + w(N) for t, s in keys))
        seq = w(N, len(packets), 2271560481) + b''.join((w(i, c) + bytes([p]) + w(N, N, N, N, N) for i, c, p, n, pr, ct, ci, ii in packets)) + seq[12 + 29 * len(packets):]
        seq = seq[:40 + 29 * len(packets)] + w(N) + seq[44 + 29 * len(packets):]
    if variant == 'trailing_nul_strings':
        strings = [s + b'\x00' for s in strings]
    if variant == 'empty_string_table':
        return container(stream, fixture_blocks(stream, 'all_strings_null'), [])
    return ([('NiTransformController', controller), ('NiControllerSequence', seq), ('NiTransformInterpolator', transform), ('NiTextKeyExtraData', text), ('BSAnimNotes', b''), ('NiTransformData', b''), ('NiNode', b''), ('FutureAnimationSource', b'opaque')], strings)

def fixture_blocks(stream, variant):
    return fixture(stream, variant)[0]
