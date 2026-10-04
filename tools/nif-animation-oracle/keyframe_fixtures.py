"""Independent authored key source values; no decoder or interpolation oracle."""
from fixtures import fixture, container, w, h, N

VARIANTS = ("quat1", "quat2", "quat3", "quat5", "xyz", "empty_xyz", "empty", "groups1", "groups5")


def group(channels, tag, values):
    if not values:
        return w(0)
    result = w(len(values), tag)
    for time, value in values:
        result += w(time, *value)
        if tag == 2:
            result += w(*([0x3f80_0000] * channels), *([0xbf80_0000] * channels))
        if tag == 3:
            result += w(0x3e80_0000, 0xbf00_0000, 0x8000_0000)
    return result


def payload(variant):
    times = [0x4000_0000, 0x8000_0000, 0x4000_0000]
    vector_values = [(time, [0x3f80_0000, 0x8000_0000, 1]) for time in times]
    scalar_values = [(time, [0xff7f_ffff]) for time in times]
    if variant == "empty":
        return w(0, 0, 0)
    if variant.startswith("quat"):
        tag = int(variant[-1])
        result = w(3, tag)
        for time in times:
            result += w(time, 0, 0x8000_0000, 0x7f7f_ffff, 0xff7f_ffff)
            if tag == 3:
                result += w(1, 0x8000_0000, 0x3f80_0000)
    elif variant == "empty_xyz":
        result = w(1, 4, 0, 0, 0)
    elif variant == "xyz":
        result = w(1, 4) + group(1, 2, scalar_values) + w(0) + group(1, 3, scalar_values)
    else:
        result = w(0)
    tag = int(variant[-1]) if variant.startswith("groups") else 2
    return result + group(3, tag, vector_values) + group(1, 3 if tag == 2 else tag, scalar_values)


def source(stream, variant, replacement=None):
    blocks, strings = fixture(stream, "baseline")
    blocks[5] = ("NiTransformData", payload(variant) if replacement is None else replacement)
    # One extra opaque branch stays an unparsed dependency in schema2.
    blocks.append(("NiBSplineCompTransformInterpolator", b"opaque-not-admitted"))
    blocks.append(("NiTransformController", w(N)+h(0)+w(0x3f80_0000,0,0,0x3f80_0000,N,8)))
    return container(stream, blocks, strings)
