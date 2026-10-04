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
