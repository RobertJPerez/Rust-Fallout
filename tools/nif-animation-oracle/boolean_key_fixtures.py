"""Independent zero-count/tag5 source bytes, with raw values and source order."""
from fixtures import N, container, fixture, h, w

VARIANTS = ('baseline','empty_group','raw_values','signed_zero','finite_extremes','duplicate_times','descending_times','unknown_links')

def expected(variant):
    if variant == 'empty_group': return dict(declared_keys=0,key_type=None,keys=[])
    pairs = [(0,255),(0x40000000,2),(0x40800000,0)]
    if variant == 'raw_values': pairs = [(0,0),(0x3f800000,1),(0x40000000,2),(0x40400000,255)]
    if variant == 'signed_zero': pairs = [(0x80000000,2),(0,255)]
    if variant == 'finite_extremes': pairs = [(0xff7fffff,0),(0x7f7fffff,255),(1,2)]
    if variant == 'duplicate_times': pairs = [(0x40000000,0),(0x40000000,1),(0x40000000,255)]
    if variant == 'descending_times': pairs = [(0x40800000,255),(0x40000000,2),(0x80000000,0)]
    return dict(declared_keys=len(pairs),key_type=5,keys=[dict(time_bits=t,raw_value=v) for t,v in pairs])

def payload(variant):
    if variant == 'empty_group': return w(0)
    times, values = [0,0x40000000,0x40800000], [255,2,0]
    if variant == 'raw_values': times, values = [0,0x3f800000,0x40000000,0x40400000], [0,1,2,255]
    if variant == 'signed_zero': times, values = [0x80000000,0], [2,255]
    if variant == 'finite_extremes': times, values = [0xff7fffff,0x7f7fffff,1], [0,255,2]
    if variant == 'duplicate_times': times, values = [0x40000000]*3, [0,1,255]
    if variant == 'descending_times': times, values = [0x40800000,0x40000000,0x80000000], [255,2,0]
    return w(len(times),5)+b''.join(w(t)+bytes([v]) for t,v in zip(times,values))

def source(stream, variant, replacement=None):
    blocks, strings = fixture(stream,'baseline')
    blocks[5]=('NiTransformData',w(0,0,0))
    target = 11 if variant == 'unknown_links' else 10
    blocks.extend([
        ('NiBoolInterpolator',bytes([2])+w(target)),
        ('NiBoolTimelineInterpolator',bytes([255])+w(target)),
        ('NiBoolData',payload(variant) if replacement is None else replacement),
        ('FutureBoolData',b'opaque-source'),
        ('NiTransformController',w(N)+h(0)+w(0x3f800000,0,0,0x3f800000,N,8)),
        ('NiTransformController',w(N)+h(0)+w(0x3f800000,0,0,0x3f800000,N,9)),
    ])
    return container(stream,blocks,strings)
