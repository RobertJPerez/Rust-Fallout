# Explicit source route overlays

`navigation::overlay::RouteOverlay::prepare(&graph, &request, limits)` resolves
caller-supplied blocked triangle IDs, blocked directed edges and finite,
nonnegative edge penalties against one existing source graph. The opaque result
borrows that graph. It stores sorted private indices and two receipt hashes;
it has no public constructor, Clone, Deserialize or graph replacement operation.
`RouteGraph::route_with_overlay(start, goal, route_limits, &overlay, query_limits,
&base_policy)` checks the exact graph address before any lookup or policy call,
then uses the unchanged route engine. A different graph refuses, including one
with the same keys, geometry and source hashes. Receipts are observations, never
deserialized graph or scope authority.

A directed edge selection requires its source TriangleId, raw `source_edge`
slot and exact `target`. JSON must include `target`, even when explicitly null.
The source slot is distinct from its compact neighbor-vector position. Parallel
edges with equal endpoints remain separate. A resolved target ID absent from the
selected graph also differs from an unresolved null source target. Preparation
rejects unknown source triangles/slots, mismatched targets, duplicate rows and
an edge selected for both exclusion and penalty. A blocked node can also have
incident penalty rows; node exclusion still wins.

Excluded start or goal fails before search. Every source and resolved destination
is checked, so an excluded destination cannot be reached by another edge. For
eligible nodes, the base policy runs before edge exclusion or penalty. Its None
and unavailable-capability error stay authoritative; a penalty cannot enable an
unknown special link, door or other rejected behavior. Invalid base costs still
fail atomically even on a blocked edge. Admitted finite costs receive only the
explicit penalty. Overflow refuses the whole route. The caller supplies a stable,
bounded immutable policy; this API bounds invocation and lookup work rather than
arbitrary caller execution or wall time.

The source scope hash streams actual graph nodes, annotations, raw record source
SHA-256 values, links and resolved target indices in graph order. The overlay
policy hash includes that scope and normalized private selections; penalties are
hashed by their binary64 words, preserving signed zero. Row permutation preserves
the policy hash. The borrowed serialization view shows actual admitted source
IDs/links, penalty values and words, and admission counters. Base policy inputs
remain separately explicit; the CLI also reports the complete request hash.

Default engineering ceilings are 10,000 rows, 4 MiB requested identity bytes,
4,096 bytes per source identity, 8 MiB retained overlay storage, 10 million
preparation visits and 64 MiB streamed source/policy hash payload. The hash tags
have fixed lengths independent of input. Source BTree lookups reserve eleven
comparisons per conservative bit-depth level, including root/miss allowance.
Sorting reserves sixteen times row count times conservative bit depth. Row,
edge, duplicate, conflict, graph and door visits share one preparation allowance.
Private index vectors and the two hash strings are reserved before allocation;
source identity strings are borrowed rather than copied into the overlay.
Query lookups share one 20-million-comparison allowance across endpoint checks,
every source/target lookup, raw link-address comparisons and binary row searches.
Ceilings may only be lowered. Existing route work/path/diagnostic limits also
apply. Exact and one-under boundaries are tested. Failure exposes no partial
route and leaves the immutable prepared overlay reusable.

`fallout navigation-overlay --install INSTALL --load-order ORDER --request JSON`
accepts the existing explicit CELL set and route policy, plus a required `overlay`
object with `blocked_nodes`, `blocked_edges` and `penalties` arrays. Optional
`source_limits`, `graph_limits`, `route_limits`, `overlay_limits` and
`query_limits` may lower ceilings. Penalty rows use `{ "edge": SELECTION,
"penalty": NUMBER }`. It loads the protected source cohort once through existing
RecordStore/NAVM decoders and builds one graph. Source selection, conservative
graph/route storage, fixed report overhead and overlay storage share one metadata
ledger before allocation. Request input is capped at 1 MiB and complete pretty
JSON plus newline at 64 MiB. Borrowed report serialization finishes while the
local Store, CellSet, graph and overlay remain alive. No graph JSON parser or
geometry rebuild supplies authority.

Tests use an independently specified diamond with parallel source slots, a
reverse cycle, door annotations and an isolated node. Frozen CLI fixtures encode
the same literal topology in authored plugins, including special/missing edges,
source changes, physical master overrides, source refusals and cache reuse.
Coincident triangles isolate topology and policy; these fixtures do not prove
original navigation geometry or actor travel. Geometry, authored units and raw
portals remain unchanged. No nearest obstacle projection, dynamic avoidance,
canonical state/save mutation, original door behavior or gameplay readiness is
claimed; `faithful_ready` remains false.
