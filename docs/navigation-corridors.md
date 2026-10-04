# Source-qualified local triangle corridors

`fallout navigation-corridor --install INSTALL --load-order order.json
--request request.json --output report.json` generates an engineering route inside
one protected source CELL and validates its required raw triangle/portal chain.
The strict request contains `cell`, `route` and `plane`. The route uses the same
explicit costs and record/triangle/door eligibility as navigation-cell-set, without
NAVI fields. The plane is `{"contract":"axis_aligned_dyadic","normal_axis":2}`
for an XY source plane; axes 0/1 select YZ/ZX respectively.

The sealed `navigation::corridor::CorridorQuery` constructor takes the existing
RecordStore, exact CELL FormKey and InputLimits. It preflights with the existing
NAVM decoder, then uses existing load_cell and RouteGraph. It generates the Route
internally and privately derives VerifiedCorridor. Neither a serialized Route nor
a caller's vertex/portal table can construct that certificate. The immutable owner
describes its loaded source snapshot; a report is an observation rather than
authority for current source, residency, world generation or canonical movement.

Each selected triangle retains its source FormKey and raw vertex indices, exact
binary32 words, vertices, record/source/decoded SHA provenance, record offset,
raw edge annotations, triangle/cover/record flags and cover membership. The chain
preserves source triangle IDs, including nonsequential IDs. Each local portal
retains source/target IDs, directed raw edge indices and exact source vertices.
The target must encode the reciprocal local edge with reversed vertex identities.
The selected triangles must have consistent winding and opposite third-vertex
sides at every shared edge. No coordinates are snapped or aligned across frames.

All selected vertices must have one exact constant coordinate on the requested
normal axis. Signed zeros compare as equal plane coordinates and remain distinct
in the retained source words. Projected binary32 values are decomposed into signed
integer mantissas and binary exponents. After selecting the smallest nonzero
exponent, all coordinate magnitudes must fit 30 bits. Checked i128 determinants
then certify nonzero area and portal sides without floating point rounding.
Finite large/subnormal values qualify when their relative exponent domain fits;
a wide exponent spread explicitly refuses. Consistent reversed winding qualifies.
This is an axis-aligned local chain certificate; no general 3D or global polygon
simplicity certificate is provided.

An unavailable/unreachable route keeps its exact existing Route observation and
has no corridor. A chosen external/special edge or any selected door triangle
refuses geometric traversal even if the caller declared graph costs. Invalid,
degenerate, repeated, noncontiguous, nonreciprocal, inconsistent-plane/winding or
unrepresentable domains cannot return a verified corridor. Door bytes remain in
the Route/source graph's observations when refusal is relevant.

InputLimits default to a 4-GiB whole source cohort, four million index/name/winner
visits, 10000 meshes, 64 MiB of decoded record read work, 100000 fields and one
million source elements. Preflight and final decoding both consume the same work
allowance. The source/graph/frontier/path reservation is global, with a 64-MiB
runtime ceiling. It counts decoded payload copies, source vector capacities,
field/identity descriptors and actual graph N/E/D before final source loading and
graph construction. Reservations are logical conservative bounds, not measured
allocator peaks. Existing decoder limits bound transient preflight arrays.

The CLI uses `source_limits`, `graph_limits` and `route_limits` from the explicit
cell-set schema. `source_limits.identity_metadata_bytes` bounds the complete
source/graph retained reservation and defaults to 16 MiB. One CELL is selected;
`cells=0` refuses. Index opening and whole-cohort admission reuse the existing
bounded Store/cache path. `corridor_limits` defaults are 10000 triangles, 100000
validation visits, 2 MiB raw geometry and 16 MiB copied identities. Limits may
only decrease. Geometry/identity space is admitted before result allocation;
source joins, vertex reads, cover entries and reciprocal edge/orientation checks
consume one validation allowance. JSON requests are capped at 1 MiB and complete
pretty reports plus newline at 64 MiB before protected create-new emission.

Literal authored fixtures route triangle IDs 4/1/5/2 through three portals at
caller cost 3. They preserve negative-zero source words, raw cover and disabled
record flags, and reciprocal portal indices. Tests include reversed winding,
subnormal/large exact dyadic values, cyclic axis selection, one-triangle routes,
global exact/one-under limits, late malformed sources, missing neighbors and
explicit external/door/plane/domain refusals.

Validation passed 21 distinct Rust checks, affected all-target Clippy with warnings
denied, formatting and the CLI build. The frozen CLI passed 65 authored cases:
9 verified corridors, 11 reports with no corridor and an explicit refusal,
43 failures with no report, and 2 exact prior-command report comparisons.
Cold/warm cache reports agree and source fixture bytes remain unchanged. Evidence
is in `local/phys20-corridor-01`; the initial wrong load-order fixture and its
failed attempt are preserved, with the corrected export and successful proof
in separate directories.

`faithful_ready=false`. This slice establishes no endpoints, smoothing, funnel,
actor package traversal, original units/costs/door behavior, canonical state/save
changes, movement commit or gameplay acceptance.
