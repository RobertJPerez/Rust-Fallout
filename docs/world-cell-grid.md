# Explicit source CELL grid requests

`world::cells::CellGridSources::load(store, world, limits)` indexes winning CELL
definitions whose own source group-world label resolves to the requested live NV
WRLD identity. It uses the existing master resolver, protected record store and
strict CELL decoder. It does not apply WNAM inheritance or infer a world from a
position. The WRLD body remains deferred; its header and full plugin fingerprint
are retained as source evidence.

`metadata()` borrows the ordered source receipts and entries in source/header
order. Each entry retains its canonical key, winning header, source ordinal,
raw parent-world label, decoded CELL hash and typed DATA/XCLC/FULL source fields.
The CELL header's persistent bit is `0x400`; DATA bit zero declares an interior.
These fields follow the pinned xEdit FNV definitions, rather than a runtime
activation measurement. Deleted winners remain header-only tombstones. Persistent
groups, interior cells and entries without XCLC remain distinct from grid cells.

`request([x, y])` accepts explicit signed integer XCLC coordinates. A unique live
nonpersistent exterior CELL produces a private `CellGridRequest`; missing or
multiple winning matches refuse. A moved winning override uses its own grid and
world/master context. An older source definition never fills a missing candidate.
No position-to-grid conversion, unit scale or default coordinate is applied.

`prepare_cell(store, request, mounts, model_limits)` checks the request's directory,
world and exact ordered source seal before using the existing `CellModelPlan`.
Changed names, order, file bytes or source count refuse reuse. The prepared plan
can be submitted to existing `CellResidency`; it does not incorporate persistent
world objects into the spatial cell or change canonical loaded state. Runtime,
terrain, collision, behavior and original activation admission remain separate.

The caller may lower these fixed source-inspection ceilings:

| Allowance | Ceiling |
| --- | ---: |
| Source plugins | 256 |
| Winning headers scanned | 1,000,000 |
| CELL entries in the explicit world | 65,536 |
| Each stored/decoded CELL body | 4 MiB |
| Aggregate max(stored, decoded) CELL reads | 64 MiB |
| Conservative retained metadata | 32 MiB |

Metadata admission precedes receipt/key collection and typed field copies.
Temporary world-key resolution is checked against remaining metadata. Counts and
stored/decoded lengths refuse before collecting or reading an oversized batch.
Malformed, tainted or over-quota requested cells produce no directory. The
retained-byte estimate excludes allocator overhead and the caller's existing
record index; source fingerprinting is separate from CELL body read accounting.
`runtime_ready` stays false.
