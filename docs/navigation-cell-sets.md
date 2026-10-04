# Explicit source CELL navigation sets

`fallout navigation-cell-set --install INSTALL --load-order order.json
--request request.json --output report.json` loads exactly the requested live
CELL FormKeys and their winning NAVM records into one graph. It does not discover
neighbors by coordinates or load cells inferred from external links.

The strict request contains nonempty unique `cells` and optionally `route`.
Route fields are `start`, `goal`, `local_cost`, nullable `portal_cost`,
`special_costs`, `allow_disabled_records`, `triangle_forbidden_mask` and
`permit_door_triangles`. Costs must be finite and nonnegative. An absent declared
portal or special-link policy remains Unsupported. Door eligibility is an
explicit caller choice; source door bytes remain annotations. NAVI observations
remain with the existing single-cell command.

`navigation::load_cells` takes the existing protected RecordStore, exact FormKeys
and lowerable CellSetLimits. It validates and reads each selected live CELL,
scans winning definitions once, and uses the existing NAVM reader and physical
plugin master binding. Each DATA cell must match the selected GRUP cell. Inputs
and meshes are returned in stable source-key order. A late bad record, duplicate
identity or exhausted budget returns no complete selection. Existing `load_cell`
and the schema1 single-cell CLI preserve their signatures and behavior.

Source defaults are 64 cells, 10000 meshes, four million index/name/master visits,
4 GiB of whole plugin cohort digest inputs, 64 MiB of total selected CELL/NAVM
payloads, 100000 total fields, one million total vertices/triangles/external edges/
cover entries/doors, and 16 MiB of identity metadata. All limits are global across
the set. Exact boundary and one-under checks include selected CELL bodies, rather
than admitting only their headers and renewing NAVM limits per cell.

Metadata reserves base, source names, selected keys and CELL observations before
allocation. The existing private mesh load path has an optional bounded hook:
field descriptors and source/target identity tables reserve metadata before
resolved external/door strings are collected. Legacy load_cell uses its previous
unmetered hook. Element and payload admission still use the existing decoder.

The CLI also validates the plugin cohort before opening the Store/cache, divides
one total four-million-record/4-GiB index decode allowance across the selected
plugins, and tightens each record read to the selected record allowance. It builds
one graph under the existing global defaults: 100000 nodes, 300000 edges and
100000 door links. Route defaults remain 100000 expansions, 300000 edge tests,
10000 path nodes and 10000 diagnostic entries. Optional `source_limits`,
`graph_limits` and `route_limits` supply all named fields of their corresponding
limit object and may only lower the ceilings. The source limit object flattens
`record_bytes`, `fields` and `elements` alongside the other source fields.

Before graph construction, the CLI counts actual source nodes N, encoded edges E
and doors D. With L the longest selected source/target origin name, it reserves
`N*(768+4L) + E*(2048+6L) + D*(512+L) + 64*(E+1)` bytes from the same identity
allowance after the source loader's usage. This covers graph identity maps and
capacities, static nonnegative-policy frontier storage, and path/missing/unsupported
receipts. The existing zero-heuristic solver remains unchanged. Reservations are
conservative logical bounds, not measured allocator peaks. Large identities or
graphs can explicitly refuse before graph construction.

The typed report owns its source set and graph and serializes graph nodes by
borrow, avoiding a second node collection. Request JSON is capped at 1 MiB. A
counting serializer admits the complete pretty report plus newline under 64 MiB
before shared protected create-new emission. Full source, load-order and request
SHA values and charged source/graph usage remain observations, not admission
authority for a later source graph.

Literal plugin fixtures give cells A/B and meshes 0x100/0x200. A0 links externally
to B0; B0 links locally to B1; B1 has a type2 external link to A1. Selecting only A
returns the exact missing B0 link. Selecting A and B returns A0/B0/B1/A1 at caller
cost `3+1+5=9`. Raw source portals remain `(0,0,0)/(2,0,0)` in A and
`(1002,0,0)/(1000,2,0)` or `(1002,0,0)/(1002,2,0)` in B. Their unrelated source
coordinates are not merged into one geometric corridor. Tests cover input-order
stability, global source/graph/search boundaries, unavailable special policy,
door exclusions, late invalid sources and a physical master-table override.

Validation passed 16 distinct Rust checks, affected all-target Clippy with warnings
denied, formatting, and the CLI build. A frozen CLI passed 50 authored cases:
16 successful reports and 34 atomic refusals. Cold/warm index cache reports agree,
fixture bytes remain unchanged, and the legacy single-cell schema1 route, meshes
and nodes agree with the equivalent explicit one-cell selection. The private
evidence directory is `local/phys19-cell-set-01`.

These are source graph proposals, `faithful_ready=false`. They establish no
original actor costs, doorway traversal, cross-cell funnel, dynamic avoidance,
canonical movement or gameplay acceptance.
