# Source navigation and engineering route queries

Navigation extends the existing plugin framing and strict winning record store.
It does not import a second plugin representation, generate navmesh, store actor
positions or commit runtime movement. The first consumer is a headless
selected-cell route request through authored triangle links.

`fallout_data::navigation` decodes the complete relevant FNV NAVM/NAVI definitions
at pinned xEdit `9fb016884bec138ea6c7b872cec831537d464c3e`. Relevant source sections
include both record definitions, the NVMI island flag decider, the complete common
edge-index callback/edge type and triangle flag definitions, and the array prefix
implementation (`-1` is a four-byte count, `-2` is a two-byte count). Those source
files carry MPL-2.0 notices. The independent Rust implementation preserves layout
facts without copying Pascal implementations or generated definitions.

NAVM retains NVER, DATA cell and all five counts, exact vertex floats, triangle
vertex/edge indices, flags and cover words, NVEX type/navmesh/triangle, cover
triangle indices, door references and their unused bytes. NAVI retains physical
NVMI/NVCI order, flags, mesh/location IDs, signed Y/X coordinates, approximate
location, optional authored island bounds/vertices/triangles, preferred scalar,
and ordered standard/preferred/door lists including duplicates. Every source
subrecord remains available with its decoded offset and bytes. NVGD and unknown
fields stay opaque. Version values are retained as data; decoding a field layout
does not certify behavior for every possible version or flag.

Wrong record kinds, tainted/deleted inputs, duplicate singleton fields, mismatched
counts, bad local/external table indices, nonfinite floats and vertex/door/cover
indices fail. Record bytes, physical fields and aggregate elements have explicit
limits. Island/connection variable arrays are checked against their remaining
source span and element budget before allocating. Source floats, signed zero,
topology, flags and unknown bytes are not normalized or repaired.

`load_cell` filters the existing winning NAVM index by retained CELL GRUP identity,
then strictly reads only those bodies. DATA cell must match that physical group
identity. Plugin master-relative edge/door words are resolved by `RecordStore`.
Their missing loaded neighbors remain absent; loading one cell never invents a
whole world. Aggregate retained field bytes, elements and selected mesh count are
bounded. Source plugin, source SHA-256 and decoded body digest bind each mesh.

`fallout_runtime::navigation::RouteGraph` builds a stable indexed triangle graph.
Local neighbors must share the authored edge vertices. A link retains its source
mesh/triangle/edge, source portal endpoints, external type/raw word and target
identity. A missing target remains a missing neighbor; ledges/enable portals keep
their types. Door metadata attaches to its exact source triangle. Door destinations
are not derivable from a door reference alone and are not synthesized.

Route cost/admission is an explicit caller policy over source node flags, door
metadata and link kind. A* uses `h=0`, which is admissible for every declared finite
nonnegative cost, including zero-cost portals and transitions between unrelated
cell frames. A Euclidean cross-cell heuristic would need stronger assumptions.
Strictly improving distances prevent zero-cost predecessor cycles. Stable queue
ties make equal-cost results deterministic for a fixed source graph and policy.
Graph nodes/edges/doors and route expansions/edge tests/path nodes/diagnostics are
bounded. Overflowing/negative/nonfinite costs fail; a budget never returns a partial
successful path. Results distinguish a known path, unreachable, reachable missing
neighbors and caller-reported unsupported capabilities. An unrelated unsupported
branch does not invalidate a found path that uses only admitted edges.

The CLI request supplies source endpoint triangle identities, explicit per-local/
portal/special costs, disabled-record admission, forbidden triangle flags, door
triangle admission and any NAVI identities to inspect. This is an engineering
source graph consumer. It does not decide original package eligibility, initialize
actors, perform funnel smoothing, avoid dynamic obstacles, cross doors or move
through collision. Faithful readiness remains false. Such behavior needs measured
policy and runtime/world/physics consumers, rather than treating authored topology
as accepted gameplay.

`fallout navigation-route --install INSTALL --load-order ORDER --editor-id CELL
--output NEW_REPORT` inspects the selected cell's source meshes and endpoint IDs.
Adding `--request REQUEST.json` runs the route. A request explicitly supplies:

```json
{
  "start":{"mesh":{"profile":"nv-original","origin_plugin":"example.esm","local_id":291},"triangle":0},
  "goal":{"mesh":{"profile":"nv-original","origin_plugin":"example.esm","local_id":291},"triangle":3},
  "local_cost":1,"portal_cost":null,"special_costs":{},
  "allow_disabled_records":false,"triangle_forbidden_mask":8,
  "permit_door_triangles":false,"navi_forms":[]
}
```

The IDs above illustrate the schema; select real IDs from the inspection report.
The request digest and source receipts bind the resulting report. A found path is
an engineering adjacency result with that explicit policy, not accepted travel.
