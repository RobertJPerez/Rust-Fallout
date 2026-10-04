# Authored door destination requests

`world::doors::DoorDestination::load(store, source_cell, source_door, limits)`
binds one explicitly requested winning door reference to its authored destination.
It reuses the existing bounded dependency graph, placement decoder, master-table
resolver and source receipts. Only a live winning REFR member of the requested
CELL, with a live DOOR base, can expose a destination. The authored XTEL target
must resolve to a live REFR with a live DOOR base and a resolved parent CELL.

`metadata()` and `graph()` borrow immutable source evidence. Node and edge indices
identify exact winning headers, physical field spans, raw form IDs and source
plugin digests. The destination position and rotation come from the source door's
XTEL field, in original units, with its raw flags preserved. Target-door DATA is a
distinct placement and is never substituted for that authored transform.
`authored_transform_words` preserves the six exact XTEL f32 bit patterns in
position XYZ, then rotation XYZ order. Integer words carry signed zero through
numeric display/JSON tools that may normalize it; they do not apply a coordinate
conversion or an activation rule. The typed/display transform remains alongside
those words, backed by the same immutable decoded source field and graph span.

Unresolved, deleted, wrongly typed, moved-out-of-cell and absent-link requests
retain diagnostics and cannot prepare a destination CELL. Construction itself
refuses framing, integrity, decoding and quota failures. Reciprocal links can
form normal source graph cycles: component indices remain explicit, and this
producer follows one XTEL rather than composing a chain of teleports.

`prepare_cell(store, mounts, model_limits)` checks the complete ordered plugin
receipt seal before invoking the existing `CellModelPlan::load` for that exact
destination CELL. A new source file, changed bytes or changed source order cannot
reuse an old request. The resulting plan can be submitted to existing
`CellResidency::request`; existing coverage, source jobs, texture requirements and
collision/behavior readiness checks still apply.

The destination CELL remains a header-only terminal in this source graph. Its
body and children are read through the existing destination model-plan consumer,
without expanding all neighboring cells during door selection. The graph's
existing limits apply, including the added conservative receipt metadata charge
of 4,096 bytes plus eight times the longest origin-plugin name. Admission precedes
the producer's owned key/receipt copies. Exact-bound and one-below-bound cases
exercise this shared quota.

This is an engineering source request. It does not apply a pose, unload a current
cell, activate a destination, execute an object script, evaluate locks/enablement
or mutate canonical state. `runtime_ready` stays false. Original activation,
orientation conventions, unit measurements and faithful traversal remain open.

The executable consumer uses `DoorPrefetcher`, with one protected ordered store
retained through source selection, complete model/texture jobs, cancellation,
actual pin drain and retry. See [the adapter's scope and limits](world-door-sources.md).
The adapter refuses cyclic destinations while this source producer preserves
their diagnostic graph components:

```powershell
fallout --output NEW_LOCAL_REPORT.json door-residency-sources --install INSTALL --load-order ORDER.json --cell SOURCE_PLUGIN:SOURCE_CELL_HEX --door SOURCE_PLUGIN:DOOR_REF_HEX
```

Optional index/resource caches and the per-stage source polling deadline retain
the existing cell consumer's bounds. The report adds `door_destination` and
`door_source_graph` to the target source receipts and residency snapshot.
Unresolved selection/planning produces its source diagnostics and a nonzero exit;
there is no residency request when the destination plan cannot be constructed.
`destination_applied` and `current_cell_changed` remain false. Successful payload
extraction still leaves collision, behavior and dependency readiness Pending.

The frozen consumer resolved the original house door `FalloutNV.esm:103e61` to
the authored target `FalloutNV.esm:103e69`. Its indexed parent is
`FalloutNV.esm:846ea`, with header flag `0x400` (persistent) and world context
`FalloutNV.esm:da726`. Destination model planning correctly refused at the
existing 4,096-node bound before creating residency. All ten captured original
plugin fingerprints remained unchanged; no original game process was launched.

The current request addresses the indexed parent CELL. This persistent world
group needs a separate spatial residency scope; the source parent alone does not
establish the destination's spatial CELL. Neither the source pose nor this
refusal authorizes an inferred grid, a raised quota or a live transition.
