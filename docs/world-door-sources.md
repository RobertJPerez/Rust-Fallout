# Exact door destination source prefetch

`world::residency::DoorPrefetcher` consumes the existing sealed `DoorDestination` into one active source request. It reuses destination cohort validation, protected `CellModelPlan` construction, `CellResidency`, the existing NIF texture-reference reader, archive importer, resource jobs and private cache.

`request` refuses unresolved, deleted, moved-out-of-requested-CELL, wrongly typed, tainted or cyclic source destinations before creating a live destination request. The underlying door producer still preserves reciprocal source cycles as evidence; this prefetch adapter conservatively leaves those cycles unsupported. A moved target may use its actual winning parent when the existing producer resolves it. The destination is its indexed source parent CELL, including a persistent group; no spatial destination is inferred.

The source identity binds the door metadata and destination model-plan seal. It preserves the source XTEL target, six exact position/rotation words and raw flags. Target-door DATA is retained as distinct placement evidence and never supplies the source landing pose. The identity string is metadata; the private owning epoch and `Ticket` enforce access.

`DoorPrefetchLimits` retains at most two door graph scopes. Each scope conservatively charges graph logical metadata plus decoded source-body extents and a fixed wrapper reserve before retention. Actual model/texture jobs, plans, metadata and mappings remain charged through the existing residency pools. Construction of caller-owned source graphs/plans has its existing separate factory limits; these logical retention budgets do not measure allocator or process peak memory.

After model jobs finish, `prepare_textures` invokes the existing bounded texture factory. Planning may block and belongs on the caller's existing source preparation worker. Missing or ambiguous references, unsupported reference coverage, final payload/queue refusal and job failures expose no successful `DoorPrefetchSources` lease. The complete lease borrows checked destination metadata, model sources, texture receipts and actual texture bytes. It exposes no detachable resource-bearing plan.

`cancel` advances the parent epoch immediately. Previously held destination/model/texture access is revoked, while those original scopes and resource pins stay charged until leases and in-flight work actually drain. A new `request` starts a fresh epoch; early source or retention refusal preserves an already live request. At most one request is active, with bounded retiring scopes and resources.

The existing `door-residency-sources` CLI now exercises this adapter: complete model/texture bytes, cancellation with the lease held, release/drain, and a fresh request with identical payload hashes and a later epoch. It retains source diagnostics on refusal. Optional caches and source deadlines use the existing cell consumer rules.

```powershell
fallout --output NEW_REPORT.json door-residency-sources --install INSTALL --load-order ORDER.json --cell PLUGIN:SOURCE_CELL_HEX --door PLUGIN:DOOR_REF_HEX
```

No current-cell mutation, player movement, landing-pose application, activation, fade, timing, GPU, physics or behavior publication occurs. Source prefetch is not verified retail traversal.
