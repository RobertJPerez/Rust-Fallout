# Explicit source-sphere engineering sweep

`physics::sweep::{SphereSweep, SweepScope, SweepLimits, SweepProposal}` and
`StaticScene::sweep_sphere(request, limits)` add an immutable proposal to the
existing collision scene. `fallout collision-sweep model.nif --request request.json`
is the actual consumer. The request supplies selected body blocks, reference,
attachment rows, engineering units, center endpoints, moving radius, positive
contact tolerance and aggregate limits. No World or saved transform changes.

The only admitted scope is `zero_tagged_frozen_sphere_cores`: a named authored
fixture profile. It requires raw body/filter-copy tags, broad phase, response,
motion, quality, deactivation and flags to be zero, velocities, inertia, center
and physical coefficients to be zero, and no constraints. These literal tags
have no inferred retail meaning. Unknown profiles refuse. Source Sphere cores
are the only obstacles; other shapes and nonzero wrapper margins refuse the
complete request, even when an index would cull them. Each selected leaf is
checked. A source-scoped selection does not certify the rest of a CELL.

Exact composition is checked alongside the unchanged existing ray/overlap
composition. Every intermediate multiplication and sum must be representable
exactly. Active body rotations must have `x=y=z=0` and `w=+1` or `w=-1`. Final
frames must be signed axis permutations with a uniform normal power-of-two
scale, plus a finite representable translation. Reflections preserve handedness.
The existing tolerance cannot turn a nonuniform sphere into an admitted sphere.
Sphere radii are transformed once into query units. No inverse ray projection or
component normalization changes the requested center trajectory.

Minkowski expansion follows directly from closed ball intersection: the moving
center intersects the source ball expanded by the sum of their radii. That sum
must be exactly representable; a small radius lost in addition refuses. Finite
nonzero input scalars, transformed radii, relative positions and endpoint deltas
are restricted to magnitudes `1e-100..=1e50`. Endpoint and source-center
subtractions must be exact. Underflow, overflow and wider numerical domains
refuse. Zero displacement, zero radius, initial overlap and tangency remain
explicit cases.

The exact stored endpoint segment is `start + t * (end - start)`, `0 <= t <= 1`.
The existing compensated sphere ray operates on this original unnormalized
delta as an additional refusal/estimate. A separate outward-rounded interval
certificate evaluates the exact-input quadratic:

```
A = delta . delta
B = (start - source_center) . delta
C = |start - source_center|^2 - expanded_radius^2
entry = (-B - sqrt(B^2 - A*C)) / A
```

Arithmetic is tightened to points only when TwoSum/FMA proves exactness within
the declared representability floor. Otherwise directed neighboring binary64
values enclose each operation. A negative upper discriminant proves no hit;
uncertain grazing, root range or conflicting ray estimate refuses. Exact maximum
range is closed. One-bit range cases may refuse when their interval straddles
the endpoint. Every result is clear, contact, start overlap or start tangent;
an error yields no partial proposal or successful CLI report.

Contact parameter bounds enclose entry on the original segment. The proposed
center uses a parameter within that interval and FMA component construction;
each component has a certified error relative to the original exact segment.
Separation bounds enclose both the exact trajectory point and the returned
rounded center. Except for explicit starting penetration, both must lie within
the caller's contact tolerance of the expanded surface. Distance bounds enclose
distance along the original segment. Full SourceId, material, filters, welding
and authored shell value remain attached to contacts. Overlapping uncertain
first-contact intervals from different sphere geometry refuse; exact ties and
identical geometry retain source-qualified contacts.

All source/decode/build work uses the existing bounded decoder and scene.
Additional frame metadata is fixed per admitted primitive and bounded by the
existing global primitive ceiling; shape composition work is bounded by global
shape visits. Sweep ceilings are 10,000 primitive tests, 10,000 leaf iterations,
10,240,000 predicate reservation credits and 10,000 contact candidates. Callers
may lower these ceilings. Each leaf reserves one iteration and 1,024 credits,
covering bounded admission, estimate, interval, witness and bounded candidate-sort work; this is an
arithmetic reservation, not a timing guarantee. Candidate allocation is charged
before retaining it, and every late refusal discards all candidates. The CLI
reads at most 1 MiB request and 64 MiB source, uses one aggregate build and sweep,
and counts complete pretty JSON plus newline against 64 MiB before emit.

Validation uses literal NIF bytes through the existing decoder, exact rational
closed-range predicates and independent high-precision roots/norms. Ordinary
rays/overlaps retain their behavior. The scope excludes character capsules,
stepping, slopes, response, Havok dynamics and original movement acceptance;
`faithful_ready` remains false and checkpoint 45 is unchanged.
