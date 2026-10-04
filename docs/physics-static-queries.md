# Authored static core queries

`fallout_runtime::physics::StaticScene` consumes the existing collision decoder.
Its first engineering slice supports spheres, box half extents, equal-radius
capsules, and uncompressed packed triangles, including shared shape DAGs,
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

Convex hulls and compressed packed vertices remain unsupported in this first
slice. Box/packed convex shell margins are retained but **excluded from core
queries**. MOPP code is retained by decoding; exhaustive child queries need no
interpretation of its acceleration instructions. There is no constraint solver,
body activation, dynamic response, character sweep/step controller or measured
retail unit conversion. `faithful_ready()` always returns false.

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
semantics and keeps faithful readiness false. Unsupported bodies return the exact
block/capability error. Retail reports/geometry stay under ignored `local/`.

Format facts: nifxml revision
`970a6238218a106daaeb89a61bcda0eeaf9d08c4`, complete relevant Matrix44,
bhkRigidBody/CInfo550_660, hkQuaternion, convex/capsule/box, transform/list/MOPP,
packed data and hkSubPartData definitions. nifxml is GPL-3.0 research;
no generated schema or implementation code was copied into this Rust query math.
Independent authored fixtures exercise the existing binary decoder and analytic
expected intersections. Engineering tests do not establish retail movement.
