# Implementation checkpoint 10: exterior height grids

The Rust inspector now reconstructs winning VHGT into bounded 33-by-33 grids under
a pinned ESM4 reference model. Source bytes remain separate. Stable identities
select unnamed neighbor cells, and edge diagnostics expose differences without
changing either surface. This advances M1; gameplay remains unfinished.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 111 tests and Clippy with warnings denied |
| Real fixtures | Six exterior grids across base NV and four campaign DLCs |
| Height comparison | Every f32 sample, extrema and model identity exact against a separate C++ path evaluator |
| Authored fixtures | 36 original gradients, including large and fractional/subnormal offsets |
| Negative checks | Changed derived bit fails comparison; scaled overflow rejected by Rust and C++ |
| Boundaries | Four Goodsprings cardinal neighbors with strict provenance and edge differences recorded |
| Source safety | Fresh full installation hashes match the original baseline |
| Acceptance | No rendered terrain, physics or gameplay acceptance |

The conversion adds deltas in f32 before scaling by eight. Rows restart from the
previous row's first accumulator. Tests cover analytic surfaces, rounding, numeric
bounds, four edges, large signed coordinates, absent/deleted LAND, unnamed cells,
world mismatches and ambiguous surfaces. Source positions remain f64.

The C++ executable evaluates each vertex along its own dependency paths. It is
original offline tooling informed by pinned OpenMW conventions and xEdit layouts,
not an executed upstream application or retail behavior oracle. Decompression and
canonical resolution remain outside its scope. Neighbor diagnostics do not certify
inheritance or retail stitching. Missing surfaces never acquire invented terrain.

[terrain-heights.json](terrain-heights.json) records source identities, comparisons,
boundary observations and hashes. Checkpoint-specific source/verification reports
bind the tested implementation and executable identities. Earlier immutable evidence
remains available; GPU and collision comparisons retain checkpoints 06 and 07.

Source normals, meshes/materials and retail terrain measurements remain next work.
Effective profiles, archive precedence and vanilla format exceptions still block
M1. See [terrain heights](../docs/terrain-heights.md) and [NEXT_STEPS.md](../NEXT_STEPS.md).
