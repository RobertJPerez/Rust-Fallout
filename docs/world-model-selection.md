# Explicit placed-reference model sources

WORLD37 adds an explicit selected-reference factory while preserving the existing full-CELL factory and its receipt/hash domain.

`CellModelSelection::load(Store, CELL, unique FormKeys, ModelLimits, SelectionLimits)` builds the existing protected dependency graph. Before reading selected base MODL fields it checks every requested key is a live, correctly typed REFR, ACHR or ACRE member of that exact CELL with a resolved source NAME edge. Empty, duplicate, different-profile, missing, outside-CELL, deleted, wrong-kind and unresolved selections refuse the entire request.

The private request retains the complete source cohort/root and selected coverage. `receipt()` reports each placed source identity as Selected or NotSelected. Omitted references carry no selected base or asset authority. Unsafe omitted MODL paths remain NotSelected; their source graph identity is preserved. This does not infer actor model choices, visibility or runtime existence.

`prepare(self, Store, MountIndex)` revalidates complete ordered source names, sizes and hashes, then shares the existing base/MODL/path/member factory and ArchivePool. Selected references sharing a base or archive path share the same request. A selected absent, unsafe, missing or ambiguous model source refuses all preparation before jobs begin. Existing graph, base, candidate, archive, mapping, decoded-byte and metadata ceilings still apply. SelectionLimits independently bounds up to 1024 references and 4 MiB of retained key/coverage metadata; its charges also enter the aggregate model-plan ledger.

`CellModelPlan::selection()` and the checked `ResidentPlan::selection()` borrow the sealed receipt. Selected plans use a distinct identity domain containing the selection identity. Full-CELL receipts retain their original serialized shape and identity domain. Existing full-CELL source refusal and deferred missing/ambiguous behavior remain unchanged.

The native consumer selects two instances sharing one base plus a distinct base:

```text
fallout-cli cell-scene-inputs-sources --install INSTALL --load-order ORDER --cell Base.esm:200 --reference Base.esm:300 --reference Base.esm:312 --reference Base.esm:313 --cache PRIVATE_CACHE
```

It extracts only the two existing selected model jobs, consumes raw payload hashes and reports immutable per-reference coverage. It also consumes WORLD35 environment sources beneath the same CELL epoch and records parent cancellation and actual final resource drain.

Selected model coverage is explicitly partial. It cannot establish complete CELL model coverage or dependency Ready, even if every selected payload is available. Source availability in this command refers to the explicit selection and declared environment sources, never render, collision, behavior or simulation readiness. With no `--reference`, the command uses the original full-CELL factory.

Authored checks cover three source instances sharing two jobs, unsafe omissions, stale source bytes/order, a failing last reference or member, deleted/wrong-kind/null-base selections, ambiguous archives, exact/one-under limits and unchanged ordinary full-CELL receipts. The tests and native consumer establish engineering source handling only.
