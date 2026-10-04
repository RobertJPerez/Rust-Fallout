"""Authored compact-transform source fields, independent of either reader.

Signed minima, sentinel/high-bit handles and unusual basis counts are source
values here. They do not assert a usable control-point channel or sampled pose.
"""
import struct

from fixtures import N, container, fixture, h, w

VARIANTS = ("combined", "float_only", "compact_only", "empty", "null_links", "unknown_links", "raw_handles", "max_basis")
FLOAT_BITS = (0x80000000, 1, 0x7f7fffff, 0xff7fffff, 0x3f800000, 0x3f800000)
COMPACT = (-32768, -32767, -1, 0, 1, 32767, -1)


def expected(variant):
    """Independent authored projection, without parsing produced bytes."""
    data, basis = (None, None) if variant == "null_links" else (11, 11) if variant == "unknown_links" else (10, 9)
    handles = (0x80000000, 0xffff0000, N) if variant == "raw_handles" else (0, 3, 65535)
    floats = [] if variant in ("compact_only", "empty") else list(FLOAT_BITS)
    compact = [] if variant in ("float_only", "empty") else list(COMPACT)
    return [dict(kind="compact_transform", start_bits=0x7f7fffff, stop_bits=0xff7fffff,
                 spline_data=data, basis_data=basis, translation_bits=[0x80000000, 1, 0xff7fffff],
                 rotation_wxyz_bits=[0, 0x80000000, 0x40000000, 0xc0400000], scale_bits=0x7f7fffff,
                 translation_handle=handles[0], rotation_handle=handles[1], scale_handle=handles[2],
                 translation_offset_bits=0x80000000, translation_half_range_bits=1,
                 rotation_offset_bits=0xff7fffff, rotation_half_range_bits=0x7f7fffff,
                 scale_offset_bits=0x3f800000, scale_half_range_bits=0),
            dict(kind="basis", num_control_points=N if variant == "max_basis" else 4),
            dict(kind="control_points", declared_float_count=len(floats), float_bits=floats,
                 declared_compact_count=len(compact), compact=compact)]


def payloads(variant):
    floats = () if variant in ("compact_only", "empty") else FLOAT_BITS
    compact = () if variant in ("float_only", "empty") else COMPACT
    data = w(len(floats), *floats, len(compact)) + struct.pack("<" + "h" * len(compact), *compact)
    handles = (0x80000000, 0xffff0000, N) if variant == "raw_handles" else (0, 3, 65535)
    links = (N, N) if variant == "null_links" else (11, 11) if variant == "unknown_links" else (10, 9)
    # Time inversion, zero/nonunit quaternion and finite sentinels stay exact.
    interpolator = w(0x7f7fffff, 0xff7fffff, *links,
                     0x80000000, 1, 0xff7fffff,
                     0, 0x80000000, 0x40000000, 0xc0400000, 0x7f7fffff,
                     *handles,
                     0x80000000, 1, 0xff7fffff, 0x7f7fffff, 0x3f800000, 0)
    basis = w(N if variant == "max_basis" else 4)
    assert len(interpolator) == 84
    return interpolator, basis, data


def source(stream, variant, replacements=None):
    blocks, strings = fixture(stream, "baseline")
    blocks[5] = ("NiTransformData", w(0, 0, 0))
    interpolator, basis, data = payloads(variant)
    replacements = replacements or {}
    blocks.extend([
        ("NiBSplineCompTransformInterpolator", replacements.get("interpolator", interpolator)),
        ("NiBSplineBasisData", replacements.get("basis", basis)),
        ("NiBSplineData", replacements.get("data", data)),
        ("FutureSplineSource", b"opaque-future-source"),
        ("NiBSplineCompPoint3Interpolator", b"unadmitted-point3"),
        ("NiTransformController", w(N) + h(0) + w(0x3f800000, 0, 0, 0x3f800000, N, 8)),
        ("NiTransformController", w(N) + h(0) + w(0x3f800000, 0, 0, 0x3f800000, N, 12)),
    ])
    return container(stream, blocks, strings)
