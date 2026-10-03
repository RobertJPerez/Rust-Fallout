# Implementation checkpoint 12: authored terrain texture dependencies

Rust now inspects the authored nonnull LAND -> LTEX -> TXST -> archive-member chain.
Winning source records remain immutable and carry offsets/hashes. Shared texture
paths deduplicate byte reads, while missing/deleted targets, unsafe paths, ambiguity
and corrupt cache entries remain diagnostic failures. This advances M1 without
claiming its completion or playable Fallout.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 129 tests and Clippy with warnings denied |
| Source fixtures | Six base/DLC cells; 83 record appearances, 59 unique tagged bodies; cached/uncached equality |
| Field oracle | Selected WRLD/CELL/LAND/LTEX/TXST fields exact against original C++ projection |
| Geometry regression | Six source meshes compared under the checkpoint 11 model |
| Archive oracle | All 40 unique selected texture members match ba2 exactly: 9,530,352 decoded bytes |
| Cache | Cold/warm receipts verified; damaged copied cache rejected with CLI exit 1 |
| Negative comparison | Altered authored texture-path byte rejected with CLI exit 1 |
| NULL layers | Four valid default references remain explicit and unapplied |
| Source safety | All 464 original files, totaling 9,907,238,722 bytes, match the baseline |
| Acceptance | Dependency inspection only; no textured terrain or gameplay acceptance |

The tested implementation is [dfc67df](https://github.com/RobertJPerez/Rust-Fallout/commit/dfc67dfb9844fd046587eb3d6e00714ebeb9253b).
Its source snapshot contains 102 files. Across the six cells, 108 layer bindings
include 64 texture-record appearances (41 unique LTEX/TXST bodies) and four valid
NULL defaults. The 40 authored texture members deduplicate across cells; all source
and decoded-byte hashes agree with the independent reader.

LTEX fields preserve material/friction/restitution bytes, specular exponent and grass
references. TXST paths preserve missing/empty values, original bytes, flag bits and
unhandled data. References resolve in the winner's source-local master table; older
texture fields never reappear through arbitrary body merging. Tight read limits
apply before extraction/decompression, and reports preserve every source candidate.

The C++ oracle is original field projection from pinned schema facts; it starts
after Rust decompression and is not xEdit or a retail application. The member oracle
uses ba2 for selected archive bytes, sharing only file guards/hashing helpers with
the project. It does not certify plugin linking, mount policy, image pixels or shaders.

[Terrain texture evidence](terrain-textures.json) and checkpoint-specific verification
and source reports bind the tested implementation and actual executables. Prior
immutable reports remain available, including checkpoint 11's GPU captures. Those
captures were not repeated or changed to claim landscape texture rendering.

Defaults, world inheritance, grass assets, texture blending, water, props, streaming,
physics and gameplay remain open. Effective profiles, archive precedence and vanilla
format exceptions still block M1. See [texture inspection](../docs/terrain-textures.md)
and [NEXT_STEPS.md](../NEXT_STEPS.md).
