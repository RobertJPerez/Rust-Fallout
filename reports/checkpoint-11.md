# Implementation checkpoint 11: source terrain meshes and GPU preview

Rust now builds bounded meshes from strict winning LAND data and displays real
Goodsprings/base/DLC surfaces through the separate Bevy inspection adapter. Authored
normal/color bytes and source receipts remain available. This advances the content
pipeline; it does not complete M1 or recreate playable Fallout.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 117 tests and Clippy with warnings denied |
| Independent geometry | Six real surfaces: 6,534 vertices/normals and 12,288 triangles exact against original C++ tooling |
| Colors | 5,445 authored triplets; TownCenter's absent VCLR remains explicit |
| Original fixtures | All 16 hide masks, coverage, winding, alternating diagonals, signed normals and invalid inputs |
| Negative comparison | Reversed triangle winding rejected |
| GPU smoke | Five real terrain captures and one Doc Mitchell interior regression |
| Source safety | All 464 installation files match the original full baseline |
| Acceptance | Unlit inspection only; no retail rendering or gameplay acceptance |

The mesh model uses pinned height integration and checkerboard diagonals, with
positive-source-Z fronts chosen for inspection. XCLC's padding bytes are preserved
and excluded from the hide mask. Authored normals are normalized without neighbor
replacement or corner repair. The renderer rebases in f64 before axis rotation and
f32 conversion. Missing colors use an explicitly labeled white inspection material.

The native oracle is separately authored C++ from format/reference facts, not an
executed upstream application or retail engine. It consumes Rust-decoded bodies;
compression and canonical resolution remain outside its scope. GPU success, source
counts and nonempty PNGs are smoke evidence, not retail image comparisons.

[Terrain geometry](terrain-geometry.json), [GPU preview](terrain-preview.json) and
checkpoint-specific source/verification reports bind the tested implementation and
actual binaries. Earlier immutable reports retain their evidence; original gradient,
boundary, collision and material-oracle scopes were not rerun for this checkpoint.

Textures/blending, inheritance, normal repair, water, props, streaming, physics and
gameplay remain open. Profiles, archive precedence and format exceptions still
block M1. See [geometry details](../docs/terrain-geometry.md) and
[NEXT_STEPS.md](../NEXT_STEPS.md).
