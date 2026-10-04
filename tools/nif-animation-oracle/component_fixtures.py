"""Independent authored compact float/point3 source products, not evaluation."""
from fixtures import N, container, fixture, h, w

VARIANTS = ("baseline", "null_links", "unknown_links", "raw_handles", "zero_handles", "zero_range", "signed_zero", "extreme_values")

def expected(variant):
    links = (None, None) if variant == "null_links" else (12, 12) if variant == "unknown_links" else (10, 11)
    handles = (N, 0x80000000) if variant == "raw_handles" else (0, 0) if variant == "zero_handles" else (65535, 3)
    scalar = 0x80000000 if variant == "signed_zero" else 0xff7fffff if variant == "extreme_values" else 1
    vector = [0x7f7fffff, 0xff7fffff, 1] if variant == "extreme_values" else [0, 0x80000000, 0xc0400000]
    ranges = (0, 0) if variant == "zero_range" else (1, 0x7f7fffff)
    common = dict(start_bits=0x7f7fffff, stop_bits=0xff7fffff, spline_data=links[0], basis_data=links[1])
    return [dict(kind="compact_float", **common, value_bits=scalar, handle=handles[0], float_offset_bits=0x80000000, float_half_range_bits=ranges[0]),
            dict(kind="compact_point3", **common, value_bits=vector, handle=handles[1], position_offset_bits=0xff7fffff, position_half_range_bits=ranges[1])]

def payloads(variant):
    # Keep authored bytes independent of the expected projection above.
    links = (N, N) if variant == "null_links" else (12, 12) if variant == "unknown_links" else (10, 11)
    handles = (N, 0x80000000) if variant == "raw_handles" else (0, 0) if variant == "zero_handles" else (65535, 3)
    scalar = 0x80000000 if variant == "signed_zero" else 0xff7fffff if variant == "extreme_values" else 1
    vector = (0x7f7fffff, 0xff7fffff, 1) if variant == "extreme_values" else (0, 0x80000000, 0xc0400000)
    ranges = (0, 0) if variant == "zero_range" else (1, 0x7f7fffff)
    scalar_bytes = w(0x7f7fffff, 0xff7fffff, *links, scalar, handles[0], 0x80000000, ranges[0])
    vector_bytes = w(0x7f7fffff, 0xff7fffff, *links, *vector, handles[1], 0xff7fffff, ranges[1])
    assert len(scalar_bytes) == 32 and len(vector_bytes) == 40
    return scalar_bytes, vector_bytes

def source(stream, variant, replacements=None):
    blocks, strings = fixture(stream, "baseline")
    blocks[5] = ("NiTransformData", w(0, 0, 0))
    scalar, vector = payloads(variant)
    replacements = replacements or {}
    blocks.extend([
        ("NiBSplineCompFloatInterpolator", replacements.get("scalar", scalar)),
        ("NiBSplineCompPoint3Interpolator", replacements.get("vector", vector)),
        ("NiBSplineData", w(0, 3) + b"\x00\x80\x00\x00\xff\x7f"),
        ("NiBSplineBasisData", w(0)),
        ("FutureSplineSource", b"opaque-source"),
        ("NiTransformController", w(N) + h(0) + w(0x3f800000, 0, 0, 0x3f800000, N, 8)),
        ("NiTransformController", w(N) + h(0) + w(0x3f800000, 0, 0, 0x3f800000, N, 9)),
    ])
    return container(stream, blocks, strings)
