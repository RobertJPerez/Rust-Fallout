# CELL environment source residency

WORLD35 connects the existing lighting and water factories to one live CELL source epoch. It adds no defaults, float evaluation, shader conversion, water simulation or readiness reports.

Construct `EnvironmentPlan::load(Arc<ResidentSources>, Store, MountIndex, EnvironmentLimits, LightingLimits, WaterLimits)` after the existing model jobs decode. The protected factories read the exact parent CELL and complete ordered plugin cohort. `CellResidency::request_environment(Ticket, EnvironmentPlan, Store)` revalidates that cohort at attachment and rejects a scope from another owner, even when its generation and source hash match. One retained environment scope is permitted per parent epoch.

The existing water member is submitted to the CELL's ResourceJobs, cache and generation. The standalone water-specific token contract is unchanged. Model, texture, terrain and noise jobs share worker, outstanding-resource and decoded-byte quotas. Auxiliary polling visits one pending batch per call, rotating among three bounded choices.

`environment_sources(Ticket)` returns a retained `ResidentEnvironment`. Its borrowed `lighting()`, `water()` and `noise()` accessors check the parent ticket. Lighting and water retain their separate source identities under the environment identity; literal XCLL, LTMP, LNAM, XCLW, XCWT and XNAM framing and words remain unchanged. Missing, null, empty and absent declarations remain explicit statuses. No detachable noise Artifact is exposed.

The scope charges its wrapper, both complete producer metadata receipts and the water archive mapping to the existing retained CELL ledger before publication. Environment limits can only lower fixed ceilings. These are conservative logical retained charges, not measured process memory peaks. A cancelled worker or externally held lease retains its actual scope and accounting until it drains. Unload invalidates borrowed access immediately. The shared parent token rejects late extraction, cache markers and completion. Dropping the final lease releases mappings, metadata and payload reservations.

`environment_snapshot()` exposes source state, noise request/completion status and retained environment charges without changing the existing Snapshot schema. SourceAvailable means the raw declarations and any admitted noise job are accessible; unavailable declarations remain visible. The checked `source_declarations_available()` accessor reports whether every declared template, water type and noise source is available; absent declarations introduce no defaults. Declared unavailable sources cannot satisfy dependency readiness. No source receipt establishes render, physics, behavior or simulation acceptance.

The native consumer is:

```text
fallout-cli cell-scene-inputs-sources --install INSTALL --load-order ORDER --cell Base.esm:200 --cache PRIVATE_CACHE
```

It consumes model bytes, both exact source receipts and noise bytes, then unloads while retaining model and environment leases. Its report records stale-access refusals, charges held after unload and complete release afterward. Repeated `--reference` opts into the separately sealed WORLD37 model subset; environment inputs still belong to the exact CELL.

Authored tests cover literal floating-point words including negative zero and NaN bits, an actual noise hash, foreign equal epochs, cohort mutation, wrong root, exact/one-under metadata/mapping/payload quotas, queue refusal and cancellation before/after extraction. These engineering consumers do not establish retail lighting or water gameplay parity.
