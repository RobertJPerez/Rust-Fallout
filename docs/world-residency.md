# Explicit live CELL source residency

`world::residency::CellResidencySet` keeps 1–4 explicitly supplied sealed CELL plans live under one allowance. `Admission { root, plan }` binds the requested key to the existing `CellModelPlan`. The source cohort must match every plan. Existing `CellGridSources::prepare_cells` and `prepare_persistent` perform source selection and plan construction.

`SetLimits` partitions worker, model, outstanding-resource, payload, retained-plan, logical plan metadata and mapped-file allowances once across fixed slots. No pool borrows another slot's allowance. Global workers are at most four, each occupied slot uses 1–2 existing workers, and each slot retains at most two plans. Unused allowance stays unused. The manager separately reserves conservative metadata for paths, keys, bounded admission work and snapshots; these logical quotas are not a process peak-memory measurement.

`admit` validates every root, duplicate, cohort and partition capacity before constructing any candidate. Candidates receive no IO poll and occupy distinct free partitions. A later factory refusal drops all candidates and returns no ticket prefix. `replace` first admits a candidate in a free partition; refusal preserves the old host. A successful replacement revokes only the old epoch. `remove` likewise revokes only its owned current ticket. Equality of numeric epochs or source hashes gives no authority over another host incarnation.

`poll(slot_budget)` visits 1–slot-count slots in round-robin order. Each visited host uses the existing bounded model poll, with at most eight completion inspections and eight submissions. Snapshot collection scans only the bounded slots and their already bounded source graphs. A failed host records its failure while the other cells advance.

Removed hosts retain their partition, worker pool, model artifacts, plan metadata and mappings while escaped source leases remain. A zero-model lease still retains its source-plan pin. Retirement finishes only after outstanding jobs, payload bytes, retained plans, metadata and mappings actually drain. A retired root blocks a fresh admission; explicit replacement can overlap its own active and retired incarnation.

The source consumer accepts a persistent group and up to three explicit exterior grids, or up to four exterior grids:

```powershell
fallout --output NEW_REPORT.json world-residency-set-sources --install INSTALL --load-order ORDER.json --world PLUGIN:WORLD_HEX --include-persistent --grid=-18,0 --grid=-17,0 --remove-index 1
```

It hashes bytes borrowed from every live CELL lease, holds the removed lease across revocation, verifies other cells remain accessible, then releases that lease and records the actual drain. A persistent group is selected by its source role; its identity does not imply a spatial grid. Selection and planning precede residency admission, and their existing factory bounds remain separate.

This consumer establishes source residency and lifetime behavior. It does not choose neighborhoods, infer activation distance, move a player, admit GPU resources or collision, or report simulation/gameplay readiness.
