"""Independent authored raw Boolean products; no truth or timeline semantics."""
from fixtures import N, container, fixture, h, w

VARIANTS = ('baseline', 'raw_zero', 'raw_one', 'raw_max', 'null_links', 'unknown_links', 'mixed_links', 'empty_data')

def expected(variant):
    values = (0, 0) if variant == 'raw_zero' else (1, 1) if variant == 'raw_one' else (255, 255) if variant == 'raw_max' else (2, 255)
    links = (None, None) if variant == 'null_links' else (11, 11) if variant == 'unknown_links' else (10, 11) if variant == 'mixed_links' else (10, 10)
    return [dict(raw_value=values[0], data=links[0]), dict(raw_value=values[1], data=links[1])]

def payloads(variant):
    # Byte authoring is independent of the expected JSON products above.
    a, b = 2, 255
    if variant == 'raw_zero': a, b = 0, 0
    if variant == 'raw_one': a, b = 1, 1
    if variant == 'raw_max': a, b = 255, 255
    targets = [10, 10]
    if variant == 'null_links': targets = [N, N]
    if variant == 'unknown_links': targets = [11, 11]
    if variant == 'mixed_links': targets = [10, 11]
    return bytes([a]) + w(targets[0]), bytes([b]) + w(targets[1])

def source(stream, variant, replacements=None):
    blocks, strings = fixture(stream, 'baseline')
    blocks[5] = ('NiTransformData', w(0, 0, 0))
    ordinary, timeline = payloads(variant)
    replacements = replacements or {}
    blocks.extend([
        ('NiBoolInterpolator', replacements.get('ordinary', ordinary)),
        ('NiBoolTimelineInterpolator', replacements.get('timeline', timeline)),
        ('NiBoolData', b'' if variant == 'empty_data' else b'unsupported-bool-key-payload'),
        ('FutureBoolSource', b'opaque-source'),
        ('NiFloatData', b'opaque-wrong-kind'),
        ('NiTransformController', w(N) + h(0) + w(0x3f800000, 0, 0, 0x3f800000, N, 8)),
        ('NiTransformController', w(N) + h(0) + w(0x3f800000, 0, 0, 0x3f800000, N, 9)),
    ])
    return container(stream, blocks, strings)
