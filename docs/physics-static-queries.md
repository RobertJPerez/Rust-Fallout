# Authored static core queries

`fallout_runtime::physics::StaticScene` consumes the existing collision decoder.
Its first engineering slice supports spheres, box half extents, equal-radius
capsules, certified eight-corner convex cuboids, and uncompressed packed triangles,
including shared shape DAGs,
transform/list/MOPP wrappers, and active `bhkRigidBodyT` poses. Unsupported selected
geometry fails the whole build. There is no visual mesh collider or second parser.

The scene is immutable frozen query geometry. It owns neither authoritative
reference transforms nor saved state. `BodyPlacement` carries runtime's
`ReferenceId`, source SHA-256, selected body block and complete caller-supplied
attachment `coordinates::Affine`. The caller must compose source NIF ancestry and
the central reference placement before submitting that attachment frame. The
standalone model query below does not infer a cell placement or attachment frame.

`EngineeringUnits` requires positive finite Havok-to-source and source-to-query
scales and an explicit relative transform tolerance in `0..=0.001`. Source axes
are retained. Shape/body translations are Havok units; attachment translations
are source units; query origins, distances and radii are query units. Composition
is output scale × attachment × Havok-to-source scale × active body pose × shape
wrappers. Shape Matrix44 payload vectors are columns, per pinned nifxml; ordinary
rigid bodies ignore their stored pose. Nonunit quaternions, projective/sheared/
nonuniform/singular transforms and inconsistent packed scale/radius copies fail.
Reflections are admitted without rewriting winding. Approximately orthogonal
binary32 rotations are accepted only within the caller's engineering tolerance;
sphere metric conversion has that explicitly bounded engineering approximation.

Rays have explicit unit direction and maximum distance. Closed sphere overlap
tests actual primitive geometry, including triangle edges and degenerate triangle
segments/points. Triangle rays are two-sided; parallel/coplanar and degenerate
triangles have no unique ray crossing. Solid queries starting inside return
distance zero. Hits are ordered by distance then stable source identity. Each hit
retains reference, source digest, body/shape block, deterministic DAG occurrence,
triangle index, material, source body/subpart filter words, welding and shell radius.
All filters are included in this engineering slice; no retail filter behavior is
inferred. Nonzero list overrides are explicitly unsupported.

General convex hulls and compressed packed vertices remain unsupported in this first
slice. Box/packed convex shell margins are retained but **excluded from core
queries**. MOPP code is retained by decoding; exhaustive child queries need no
interpretation of its acceleration instructions. There is no constraint solver,
body activation, dynamic response, character sweep/step controller or measured
retail unit conversion. `faithful_ready()` always returns false.

A `bhkConvexVerticesShape` is admitted only when eight finite `w=0` vertices
are exactly the eight distinct corners of a nondegenerate cuboid and six finite
outward planes cover each axis face once. Each plane's offset must equal its
normal times the corresponding vertex coordinate under source binary32 rounding.
Queries use the exact cuboid vertex hull. The authored planes can differ from
that hull by binary32 rounding; their complete words remain retained alongside
the source vertices and shell radius. This certificate uses no contact or
transform tolerance. Missing, duplicated, interior or nonaxis vertices/planes
refuse before publication. Arbitrary convex geometry never becomes its bounds.

Build limits bound input block indexing, all shape visits (including shared
paths), expanded primitive occurrences and geometry elements. Iterative traversal
does not use a source-controlled call stack. Cached geometry is shared by block;
placement/occurrence transforms remain separate. Subpart mapping takes linear
vertex/triangle work, without a triangle × subpart search. Query budgets bound
primitive tests, triangle tests and returned hits. Any exhausted budget discards
partial results. Query/local coordinates, radius and ray distance are limited to
absolute `1e50` to avoid overflowing higher-order dot/cross products; nonfinite or
overflowing conversions return errors. These limits are engineering choices,
not measured game-world limits or peak-process-memory guarantees.
Sphere and capsule rays measure perpendicular distance directly with compensated
cross products, avoiding cancellation between squared axial distances. When a
true surface entry is too small to distinguish from the closest approach in
binary64, the query refuses the numerical input. Units and transform tolerance
are immutable after scene construction; `units()` returns a copied description.
The cross product uses the original direction, followed by scalar speed division;
component normalization would change a distant skew ray. Inconclusive grazing
error intervals refuse. Capsule axis projection also refuses when
`128 * EPSILON * (origin-to-first distance + axis length) > radius`. This bounds
an engineering numerical operation and can reject valid thin/distant geometry.
Capsule initial containment uses a guarded closest-segment distance, with exact
axis clamping for axis-aligned source segments and recovered subtraction residuals.
An uncertain surface comparison
refuses instead of converting a rounded squared distance into an inside hit.
Capsule sides use the original source edge through
`|(origin-first + t*direction) x edge| <= radius*|edge|`, rather than a
normalized-axis double projection. Nonaxis comparisons include a conservative
source-scale error interval and inverse-sine direction conditioning. Near-parallel
or inconclusive side/segment-endpoint predicates refuse. Exact source-axis
projection deletes a component and preserves ordinary axis/cap cases.
Box and certified-cuboid slabs enclose subtraction and division with directed
binary64 neighbors, retaining exact operations for ordinary closed contacts.
The query's `[0,max_distance]` interval participates in the same predicate.
Only a guaranteed nonempty interval produces a hit; an unresolved comparison
refuses the whole query. The reported distance is a conservative representable
entry witness inside that interval, which may be slightly beyond the exact
surface entry. Hit positions use fused multiply-add. Zero-radius cuboid overlap
compares components directly. Positive radii use scale-safe norms and recovered
subtraction residuals, refusing uncertain contact comparisons. These local
predicates retain the declared engineering transform scope; they do not certify
retail units or arbitrary approximately similar transforms.

Scenes with at least eight certifiably enclosed primitive occurrences use a
balanced source-bound index. Its bounds enclose supported authored core geometry;
they only reject guaranteed misses and never produce hits. Identity frames have
direct outward source bounds. Other validated transforms use directed intervals
to invert the actual stored binary64 inverse matrix, then enclose the source
volume in query coordinates. Query-dependent padding covers rounding in the
existing local origin/direction/center projections and radius conversion.
Translation, rotation, reflection, uniform scale and accepted near-similarity
frames retain their original transforms and narrow predicates. Uncertifiable
transform bounds retain the exhaustive path, merged in original occurrence order.
Unresolved or overflowing bound predicates keep the source candidate.
Absolute projection and radius bounds also keep candidates whenever the original
local numerical domain cannot be certified. A distant bounding miss cannot hide
the original local-conversion refusal.
Build accounting charges every retained index node and fallback ordinal against
`geometry_elements`. A query derives a separate index-visit ceiling of twice its
initial `primitive_tests`; narrow primitives still charge the original counter.
Traversal stops before exceeding that ceiling, and any error discards the whole
query. Candidate ordering and final distance/source ordering are deterministic.
At most the visited leaf count is retained and sorted for original occurrence
order. These caps bound query work/storage; they do not guarantee pruning of
every miss. In particular, a long oblique ray may intersect many world-axis
enclosures without intersecting their authored shapes and can exhaust its budget.
Balanced construction halves each range, bounding its stack depth by
`usize::BITS`, independently of authored graph depth. Source graph traversal
remains iterative. No visual bounds or decoded MOPP instructions replace the
authored narrow geometry.

For transformed bounds, let `B,q` be the unchanged stored local affine and `F`
the enclosed mathematical inverse of `B`. Static bounds enclose `F*(source-q)`.
Each world component uses outward nonnegative coefficients
`16*EPSILON*sum_j(abs(F_ij)*abs(B_jk))` and
`16*EPSILON*sum_j(abs(F_ij)*abs(q_j))` for projection error. The existing point
and vector evaluations take at most seven and six rounded operations; this
allowance exceeds their standard binary64 roundoff bound. Nine minimum
subnormals times the absolute row sum cover underflow and radius division.
Ray padding depends on `abs(origin_k)+max_distance*abs(direction_k)`, including
the separate rounded origin and direction projections throughout the bounded
segment. Overlap padding also encloses the actual local radius using the row
sum divided by the existing validated scale, with outward division rounding.
Parent nodes take componentwise maxima of child coefficients. No culling
enclosure or padding becomes contact geometry. An interval determinant containing
zero or an unrepresentable enclosure falls back; query padding overflow retains
candidates. This numerical enclosure does not extend the scope of the original
shape predicates or certify retail geometry behavior.

## Headless source consumer

`fallout nif-collision INPUT --query-request REQUEST --output NEW_REPORT` invokes
the production decoder, scene converter and bounded queries on one read-only
NIF/blob. The request selects explicit bodies, identity, units and attachment.
For a model-local engineering fixture whose selected body is block 1:

```json
{
  "reference": 1,
  "body_blocks": [1],
  "attachment_rows": [[1,0,0,0],[0,1,0,0],[0,0,1,0]],
  "units": {"havok_to_source":1,"source_to_query":1,"transform_tolerance":0.00001},
  "ray": {"origin":[-5,0,0],"direction":[1,0,0],"max_distance":20},
  "overlap": {"center":[0,0,0],"radius":1}
}
```

The report binds both input and request SHA-256, exposes the core/frozen query
semantics and keeps faithful readiness false.
The `ray_numeric_input` field retains consumed binary64 words as hex strings,
so independent predicates can bind the actual numeric inputs after JSON parsing.
Ray refusals also include this numeric audit in stderr while producing no report.
`overlap_numeric_input` similarly retains all center/radius words for successful
queries and refusals.
Unsupported bodies return the exact block/capability error. Retail reports/geometry
stay under ignored `local/`.

Format facts: nifxml revision
`970a6238218a106daaeb89a61bcda0eeaf9d08c4`, complete relevant Matrix44,
bhkRigidBody/CInfo550_660, hkQuaternion, convex/capsule/box, transform/list/MOPP,
packed data and hkSubPartData definitions. nifxml is GPL-3.0 research;
no generated schema or implementation code was copied into this Rust query math.
Independent authored fixtures exercise the existing binary decoder and analytic
expected intersections. Engineering tests do not establish retail movement.
