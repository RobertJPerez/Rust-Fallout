# Implementation checkpoint 13: authored terrain blend maps

Rust now expands sparse quadrant alpha samples into bounded byte-weight grids,
preserving source layer identities and explicit missing/default material gaps.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 134 tests and Clippy with warnings denied |
| Real fixtures | Six base/DLC cells; exact fields, geometry and byte-weight comparison |
| Independent calculation | Original C++ tool rereads tagged bodies and evaluates each vertex |
| Negative comparison | Altered calculated weight rejected with CLI exit 1 |
| Texture bytes/cache | Selected ba2 member comparison and copied-cache rejection repeated |
| Missing/default layers | TownCenter bases absent; four DLC NULL layers remain unapplied |
| Source safety | All installation files match the original baseline |

The pinned reference informs an inspection model. Retail blending, chunk edges,
tiling, lighting, defaults and gameplay remain unverified. Source floats remain
unchanged; clamping and excess coverage are counted separately. No new GPU evidence
is claimed in this checkpoint.

See [terrain blend inspection](../docs/terrain-blends.md),
[blend evidence](terrain-blends.json) and checkpoint-specific source/verification
reports. M1 and all gameplay milestones remain open.
