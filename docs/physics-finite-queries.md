# Original finite geometry queries

`StaticScene::segment_cast(Segment {start,end}, SegmentQueryLimits)` consumes
the original binary64 endpoints and the closed parameter interval `[0,1]`.
Zero length is a certified closed point query. It uses existing authored Sphere,
Box, certified ConvexCuboid, Capsule, and packed Triangle cores, including
coplanar and degenerate triangles. It excludes shell inflation and includes all
source filters. `faithful_ready()` remains false.

Each `SegmentIntersection` carries source-qualified `Hit` metadata, raw source
core words, a parameter and exactly representable point on the original line,
directed occupied entry/exit parameter enclosures, and a distance enclosure.
The witness can occur after entry; `Hit.distance` estimates that witness's
distance, and does not claim the exact boundary distance. An independently
certified interior point is useful when an irrational entry cannot be encoded
as a binary64 point. Results sort by entry enclosure lower endpoint and source
identity; overlapping enclosures do not establish exact physical ordering.

The private parameter kernels reuse the reviewed compensated sum/product and
cuboid slab predicates. Endpoint subtraction errors remain directed intervals.
Sphere discriminants use original-direction cross products; capsule occupancy
combines the original finite cylinder with endpoint balls, and triangles use
plane and closed half-plane constraints. A second evaluation certifies the
original interpolation independently of any rounded direction. A candidate
point must also pass original source membership; uncertain arithmetic refuses
the complete query instead of producing a hit or clear-space assertion.
The five bounded entry/interior/seed/exit/start candidates are deliberately
finite: a valid geometric intersection can refuse if none of those points has
an exactly representable original interpolation, even if another point exists.
For example, a segment from X=-3 to7 through a unit sphere refuses its
nonrepresentable candidate interpolation; the dyadic endpoint span -3 to5 is
certified. This API certifies successful results rather than promising numerical
completeness across every binary64 input.

Preparation separately certifies the actual authored quaternion/matrix recipe,
unit conversions, body/attachment and shape transform composition. Query
conversion encloses the inverse of that certified forward matrix. This
certificate is independent of the restricted PHYS23 moving-sphere profile.
Approximate normality of a matrix does not certify its arithmetic, and
transforms are never normalized. Existing shape admission still applies.

`FiniteQueryLimits` has reduce-only admission, primitive, geometry, predicate,
row and logical retained-byte ceilings. All selected kinds/frames are admitted
before the exhaustive bounded scan. Whole-query selected leaf and geometry work
and a fixed 8,192 predicate-operation reservation per leaf are precharged.
Scratch loops and shape arity are fixed; there is no adaptive root search.
The result vector's `min(selected leaves,row limit)` capacity and complete
inline core DTO are charged before allocation; the byte limit describes that
logical retained capacity, excluding immutable source-scene storage, bounded
stack scratch and allocator overhead. This is not a measured peak-heap or
hard real-time guarantee. A late row, numeric or unsupported failure discards
the complete owned result.

The existing `nif-collision --query-request` command accepts exactly one
`segment_cast: {segment,limits,output_bytes}` object. Its report retains six
endpoint and fifteen explicit environment binary64 words. The pretty complete
report is counted against the reduce-only 64 MiB output ceiling before the
existing emitter can create an output file. Finite forms cannot be mixed with
other forms, and resident-cell requests explicitly refuse them. Existing
ray/overlap request and report bytes are retained.

## Closed solid ray occupancy

`StaticScene::ray_intervals(Ray, IntervalQueryLimits)` shares the bounded kernels
and returns `SolidOccupancy` rows with clipped directed entry/exit enclosures,
initial containment and an independently certified point on the original ray.
The parameter is the caller's ray parameter, with no normalization or surrogate
direction, and is clipped to `[0,max_distance]`. A surface origin is contained;
an outward surface ray can occupy only parameter zero. Zero distance is a point
query. Separate selected leaves produce separate rows, including overlaps.

This boundary admits only Sphere, Box and source-certified ConvexCuboid cores
under exactly certified signed-axis, uniform power-of-two similarity frames.
It refuses Capsule, Triangle and other frames even when they are far outside
the caller's range. A near-unit direction is accepted under the existing
explicit engineering tolerance; no exact Euclidean-distance interpretation is
inferred from that tolerance. Discriminants and slabs retain the original
direction and parameter, including distant axial cases.

The CLI accepts `ray_intervals: {ray,limits,output_bytes}` and retains seven ray
and fifteen environment binary64 words. The limits, atomic failure and output
policy are the same as segment queries. This is authored core engineering
occupancy, without material response, damage, shell/filter behavior, canonical
movement, actor state, save changes or original gameplay acceptance.

## Verification

`physics_finite.rs` tests original endpoints, closed contacts, reversed and zero
length, skew/near misses, all admitted leaf kinds, coplanar/degenerate triangles,
source transforms and units, containment/surface directions, distant rays,
global exact/one-under work and retained capacity, and late whole-query refusal.
The existing physics suites and index/CLI tests are also affected checks.

`tools/collision-oracle/finite_queries.py` writes literal NIF sources and runs a
frozen actual CLI. Its independent Fraction arithmetic checks every complete
source row, raw core/request words, original interpolation, exact source affine
inverse and membership, clipped rational/algebraic boundaries and distance
enclosures. Algebraic root comparison uses exact squared rational inequalities,
not rounded square-root expectations. It preserves refused requests and absent
outputs, budget boundaries, and legacy byte-control pairs. Failed runs remain
separate immutable evidence. Source engineering tests do not prove gameplay.
