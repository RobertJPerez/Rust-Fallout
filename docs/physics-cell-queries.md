# Generation-bound selected collision geometry

`physics::cell::CellCollision` owns one immutable selected-model scene and an
`Arc<world::residency::ResidentSources>`. Source byte and plan pins stay charged
to the existing world owner. It never clones a resident resource-bearing plan,
rereads an archive during a query, or owns canonical reference state.

`admit(&mut self, &mut CellResidency, &Ticket, model_index, &[BodyPlacement],
EngineeringUnits, QueryLimits)` reads a captured source model and compares its
SHA-256 with every placement's explicit source digest. The existing bounded
collision decoder and `StaticScene` build the selected bodies. Replacement first
releases the old scene; failed source/geometry/admission leaves no partial scene.
The owner validates the exact ticket again before admission. Public numeric
root/identity/generation metadata cannot authorize a foreign owner.

`ray_cast` and `overlap_sphere` borrow the original `CellResidency`, validate its
current source lease, and return opaque `CellHits`. `CellHits::hits` and `scope` validate
the ticket and borrows both the receipt and original owner for the returned hit
slice. Receipts have no detached serialization or unchecked hit accessor. Old
receipts refuse after unload, retry, or owner destruction. Copied
inspection artifacts retain diagnostic scope, rather than live authorization.

Call `invalidate(&CellResidency)` at unload/retry boundaries. It drops stale query
geometry and the upstream source/plan lease. A stale query also performs this
cleanup. `release(&mut CellResidency)` drops the current selection and reports
collision `Pending` only to its still-current original owner. Held hit receipts
retain no geometry or source byte pins, so they cannot prevent release.

This is selected-model engineering functionality. Caller reference IDs and
complete attachment frames are explicit inputs; canonical source-reference,
pose, enable state and revision binding remain a separate runtime consumer.
No movement or saved-state mutation occurs. `report_collision` is explicitly
`Unsupported` even after a successful selected build: whole-cell coverage,
measured units, filter/contact/shell behavior and dynamics are not certified.
Geometry admission cannot set `simulation_ready` or faithful readiness.

The existing limits bound model source bytes, decoded arrays, blocks, shape
visits, primitives, geometry elements, query work and hits. This cache holds one
selected scene; the upstream owner enforces its source/plan accounting across
unload and retry. Model decoding is one bounded synchronous operation rather
than a claim about original-game or application scheduling.

The owned headless command is:

```text
fallout cell-collision --install INSTALL --load-order ORDER --editor-id CELL \
  --request REQUEST --index-cache PRIVATE_INDEX --source-cache PRIVATE_SOURCE \
  --output NEW_REPORT
```

The strict JSON request contains `model_index`, exact captured-model
`source_sha256`, `io_deadline_ms` in `1..=60000`, optional `verify_unload`, and
`query` with the existing explicit `reference`, `body_blocks`,
`attachment_rows`, `units`, `ray` and `overlap` fields. The model index comes
from the sealed cell model plan. No source body, frame or unit is selected by
default. The deadline applies only to offline source IO.

With `verify_unload: true`, the consumer first serializes current validated
engineering hits, then unloads the source owner. It requires old receipt refusal,
zero retained physics primitives, zero source bytes/outstanding jobs and zero
retained plans before producing the inspection artifact. The original install,
saves and settings remain untouched.
