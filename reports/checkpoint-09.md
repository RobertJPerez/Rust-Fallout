# Implementation checkpoint 09: exterior source fields

The Rust headless inspector now reads exterior CELL, WRLD and LAND fields through
the existing content store. Goodsprings and a bounded set of DLC exteriors have
source provenance, strict selected-body reads and exact field comparisons.
This checkpoint advances M1; no gameplay scenario is accepted.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 98 tests and Clippy with warnings denied |
| Builds | Windows MSVC release CLI, Rust evidence runner and standalone C++ field oracle |
| Exterior fixtures | Goodsprings, GoodspringsSource and one exterior per campaign DLC |
| Comparison | Cached/uncached selected fields equal; independent schema fields exact |
| Raw terrain | Height offset bits/deltas, normal/color bytes, ordered texture layers and alpha entries retained |
| Parent worlds | Winning identities, typed links, iterative chains, cycle and depth errors |
| Negative check | Altered height-offset bit produces failed comparison and exit code 1 |
| Source safety | Fresh complete installation hashes compared with the original baseline |
| Gameplay | Terrain rendering, traversal, physics, water and scripts unfinished |

The new terrain command can use checkpoint 08's metadata cache. Each selected source
is hashed through the retained read-only handle, at most once per plugin per store.
LAND membership follows the winning definition's source-local parent labels.
Moved overrides change cells, deleted overrides stay tombstones, and complete
replacement bodies do not acquire fields from earlier definitions.

Known fields validate lengths, singleton occurrences, finite floats and quadrant/
alpha indices. Bit patterns, padding and unknown bytes remain unchanged. A bounded
framing pass estimates owned field storage before vectors are built. Parent worlds
retain their own fields; editor defaults and inheritance rules are not applied.

The independent C++ tool reads tagged strictly decoded bodies and projects the same
pinned xEdit field definitions separately. It is original offline code using the
standard library and Windows CNG. It is not xEdit or a retail runtime, and does not
independently verify decompression, canonical resolution or gameplay. The Rust
comparer verifies body hashes and exact fields and reports differing field paths.
Retail-derived bodies and detailed records remain under ignored local storage.

[exterior-fields.json](exterior-fields.json) records the actual fixture counts and
hashes. [verification.json](verification.json) and [source-snapshot.json](source-snapshot.json)
bind the tested implementation and executable identities. Earlier checkpoint
verification and source snapshots remain available; collision comparisons and GPU
captures retain their checkpoint 07 and 06 origins.

Next, verify height reconstruction, normal interpretation, terrain boundaries and
world-parent inheritance, then build exterior geometry through the existing
presentation boundary. Effective retail profiles, archive/loose precedence and
known source format defects remain M1 gaps. See [NEXT_STEPS.md](../NEXT_STEPS.md)
and [exterior inspection](../docs/exterior-fields.md).
