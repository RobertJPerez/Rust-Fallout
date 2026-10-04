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

`prepare_terrain(store, request, mounts, terrain_limits)` applies the same private
request and ordered source checks before passing the exact CELL to existing
`terrain::preparation::TextureSourcePlan::load`. Its resulting plan must retain
the directory's canonical source cohort. Existing strict LAND/world/layer/default
and external texture readers, quotas and unresolved-source diagnostics apply.
The plan can feed existing `TexturePreparation` jobs and cache; this connection
does not admit a rendered terrain surface, evaluated inheritance, collision or
canonical spatial membership. Duplicate LAND and tainted payloads remain refused.

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

`grid-residency-sources` is a read-only executable consumer of this request and
the existing model/texture jobs. Supply `--install`, `--load-order`, `--world`
(canonical origin-plugin:local-hex-id), `--grid-x` and `--grid-y` (signed i32).
Optional private `--index-cache`, `--cache` and `--source-timeout-ms` follow the
other residency source commands. The timeout bounds each worker polling stage,
separately from source indexing, fingerprinting and planning.

The report retains the directory, explicit grid, selected sealed request,
plugin/order fingerprints and exact model/texture receipts and leased texture
hashes. Missing or ambiguous selection retains its directory and source error
without making a residency request. Planning refusal similarly retains the
selected request. A completed source request uses the same protected ordered
Store and existing importer/cache; it does not create a second resource route.
An unavailable request emits its report and exits 1. Source availability does
not satisfy original lookup precedence or activation acceptance, and
`current_cell_changed`, `activation_applied` and `runtime_ready` remain false.

`grid-terrain-sources` uses the same explicit world/grid flags, protected directory
and private selection as the model command. It calls `prepare_terrain` and the
existing `TexturePreparation` job/cache pipeline, with bounded polling and
owned-request cancellation on timeout/failure. The report retains the exact
terrain source plan, source-bound texture receipt and job usage. Requested member
completion remains separate from missing/ambiguous paths and unapplied default
layers; either unresolved authored coverage or strict planning refusal exits 1.
Duplicate LAND refuses before a job request. `surface_prepared`, runtime readiness
and activation stay false; this command does not decode a DDS/material surface.

`grid-residency-sources --include-terrain` explicitly declares the strict terrain
texture plan in the same CELL epoch and shared job pool as its models and model
textures. It consumes both borrowed texture leases and retains their hashes and
CELL ticket binding in the report. Source availability requires every declared
source batch's coverage; original model gaps still refuse even if terrain completes.

The command retains model and both texture leases across `unload()`, reports stale
access rejection and unchanged retained payload/metadata/mapping charges, then
releases those leases and records the drained Unrequested state. These are source
lifetime checks. Dependency, collision and behavior reports remain Pending;
surface, render, simulation, current-cell change and activation remain unadmitted.
Omitting the flag preserves the existing model/texture request scope.

`request_set(&[[x, y], ...])` selects a nonempty, duplicate-free explicit list
in caller order. Every coordinate must select one live exterior grid CELL;
a missing, deleted, non-grid or ambiguous final selection refuses the whole
request. The private `CellGridSetRequest` retains the same directory/world/source
seal as its single requests. It performs no coordinate arithmetic.

`prepare_cells(store, request_set, mounts, ModelSetLimits)` validates that seal
and prepares a `CellModelPlanSet` through the existing factory. Its borrowed
requests and plans preserve caller order and exact single-plan identities.
The result cannot be constructed from report JSON. A refusal returns no partial
bundle and releases any plans and protected archive inputs admitted earlier.

Each factory receives the aggregate allowance remaining after earlier plans,
before reading bodies, collecting metadata or opening archive mappings. Existing
per-cell ceilings still apply. Model requests, model bytes, graph scans, source
receipts and plan metadata count per cell, including coincident model requests.
An exact container-bound protected `ArchiveInput` is reused across plans; its
immutable mapping is charged once. Container aliases are not guessed equivalent.

| Aggregate construction allowance | Ceiling |
| --- | ---: |
| Explicit grids/plans | 8 |
| Source receipt occurrences | 2,048 |
| Winning headers scanned | 8,000,000 |
| Graph nodes / edges | 4,096 / 16,384 |
| Base records / archive candidates / model requests | 1,024 / 8,192 / 1,024 |
| Field sites | 262,144 |
| max(stored, decoded) plugin-body reads | 64 MiB |
| Decoded plugin bodies / model member bytes | 64 MiB / 64 MiB |
| Conservative model probe metadata | 512 MiB |
| Conservative retained plan/set metadata | 32 MiB |
| Distinct protected archive inputs | 8 |
| Actually shared mapped archive extents | 16 GiB |

All allowances are lower-only. Set metadata is admitted before selection/source
validation copies and retained plan collection. Directory construction and plugin
fingerprinting are separate from these preparation counters. Estimates exclude
allocator overhead and the caller's existing index. This caller-owned immutable
construction budget is not a global process quota or a residency owner. Cloned
individual plans retain their already admitted shared metadata and source inputs.
The bundle creates no jobs and applies no activation or runtime readiness policy.
