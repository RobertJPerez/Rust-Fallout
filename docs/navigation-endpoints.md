# Exact selected source endpoints

`fallout navigation-endpoints --install INSTALL --load-order order.json
--request request.json --output report.json` classifies an explicit source-space
point against its selected source triangle. The strict request has `cell` and
nonempty `points`; each point has `triangle`, `point` and `plane`. For example,
`plane={"contract":"axis_aligned_dyadic","normal_axis":2}` requests the exact
XY source plane. Axes 0/1 select YZ/ZX. The source CELL and triangle FormKeys are
exact, with no nearest mesh/triangle discovery or coordinate adjustment.

`navigation::endpoint::EndpointQuery::load` borrows the existing RecordStore
mutably for the query lifetime. A private admitted SourceCohort loads the exact
live CELL/winning NAVM cohort through the existing decoder and single-cell
loader. This is the same private admission code extracted from CorridorQuery;
corridors retain their previous graph reservation/build, while endpoint queries
do not build a graph. A degenerate decoded triangle can therefore report explicit
Unsupported rather than claiming a graph route or preventing that observation.

The owner retains the ordered actual source receipts and exact winning CELL
header/source binding. Its scope SHA covers those observations. Each endpoint
retains mesh/CELL/triangle identity, physical source and decoded payload SHA,
record offset, raw indices/edges/record/triangle/cover flags, binary32 vertex words,
and explicit binary64 point words. Signed zeros remain distinct in those words.
Triangle/source observations do not apply movement eligibility or actor costs.

`inspect` returns an opaque EndpointBatch with private fields, without Serialize,
Deserialize or a public receipt constructor. `observations` accepts that batch
only from its own live private Arc authority and returns a borrowed EndpointView.
A different query generation rejects it even for identical bytes. Release/drop
ends access; the source Store cannot be replaced while the query holds its borrow.
The CLI serializes the borrowed view through a callback before dropping its source
owner. It avoids constructing an additional JSON Value or copied observation
collection. Serialized reports are observations and cannot reconstruct a usable
batch or prove current canonical world/residency state.

The existing Store owns write/delete-denying Windows handles throughout indexing,
reads and query lifetime. Cached source SHA values come from those handles. On
other platforms the existing immutable-installation contract still applies. New
or changed source/cohort owners cannot use a previous generation's batch; tests
also verify a changed full source scope and physical master-table override.

Supported geometry requires finite source coordinates in one exact constant axis
plane and nonzero exact projected area. Binary32 source values convert exactly to
binary64. Projected source/point values are decomposed into normalized dyadics and
scaled to a common exponent; coordinate magnitudes must fit 60 bits. Differences
then fit 61 bits, and checked i128 products/determinants fit without rounding.
Wide exponent domains, degenerate triangles and nonconstant planes are explicitly
Unsupported. Large/subnormal finite values qualify when this relative domain
fits. Consistent reversed winding needs no coordinate or index rewriting.

Results are Inside, OnEdge with source edge index, OnVertex with source vertex
position, Outside with `off_plane`, or Unsupported with a reason. A source area
certificate precedes containment; an exact normal-coordinate mismatch then gives
off-plane Outside. There is no height tolerance. In-plane edge-side signs certify
interior/boundary/exterior relative to the source winding. A one-bit source-space
point change can therefore change classification, with its exact words retained.

Source defaults match the existing admitted single-cell input: whole cohort
4 GiB, four million source index/name/winner visits, 10000 meshes, total decoded
read work 64 MiB, 100000 fields and one million elements. Preflight and final
decoding debit one allowance. Runtime retained source reservation is capped at
64 MiB; the CLI uses `source_limits.identity_metadata_bytes`, default 16 MiB.
The source reservation includes vector capacities, raw payload copies, descriptors
and resolved identities. Existing decoder limits bound transient preflight data;
the existing digest reader also uses its bounded buffer. Reservations describe
logical retained admission, not allocator peaks. Endpoint source loading does
not consume or build a route graph.

Optional `source_limits` uses the cell-set schema. One CELL is selected and
`cells=0` refuses. Optional `endpoint_limits` defaults to 1024 points, 100000
request/source-name/mesh/triangle/vertex visits, 10000 exact determinant tests,
512 KiB raw geometry and 8 MiB copied identities. Objects supply all named fields
and only lower ceilings. Before allocating result observations, one reservation
admits `256*N` geometry bytes and `4096+N*(4096+8L)` identity bytes, where L is the
longest selected source/request origin name. One failed late point or exhausted
allowance returns no complete batch/report. JSON requests are capped at 1 MiB;
complete pretty reports plus newline at 64 MiB precede protected create-new output.

Literal plugin fixtures select source triangle ID 3 with vertices `(0,0,-0)`,
`(2,0,0)`, `(0,2,0)`. They independently establish interior/edge/vertex/exterior,
one-bit diagonal and off-plane cases, reversed winding, cyclic axis selection,
negative coordinates, minimum binary32 subnormal and large finite scales. Tests
cover exact global/one-under limits, stale owner/changed source/cohort, source write
denial, physical master override, batch ordering and malformed source/refusal cases.

Validation passed 27 distinct Rust checks, affected all-target Clippy with warnings
denied, formatting and the CLI build. A frozen CLI passed 69 authored cases:
21 endpoint reports containing 310 observations (including a 256-point independent
rational grid), 45 failures with no report, and three exact comparisons with prior
cell-set/single-cell/corridor reports. Cold/warm cache reports agree and all fixture
bytes remain unchanged. Evidence is in `local/phys21-endpoints-01`.

`faithful_ready=false`. This proves an explicit engineering source-plane subset;
it establishes no nearest-nav selection, projected/snapped pose, original units,
actor/package behavior, corridor smoothing/funnel, canonical movement/save/state,
resident navigation readiness or gameplay acceptance.
