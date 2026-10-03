# Shared source ownership in runtime worlds

`World::new`, `World::with_campaign`, `World::restore` and repository load/recovery
accept either `&Catalogue` or `Arc<Catalogue>`. Existing borrowed callers retain
their lifetimes. An owned world can be `World<'static>` and move to a worker or
presentation component while keeping its source records alive.

`SourceCatalogue` holds the borrowed view or shared owner. Cloning it copies a
reference or increments the Arc count; it never copies a catalogue, script body
or mutable campaign bank. `World::catalogue()` exposes an immutable source view.
Prepared script plans still borrow that view and cannot outlive it. This change
does not create a self-referential plan cache or extend source borrows unsafely.

```rust
let source = std::sync::Arc::new(catalogue);
let world: fallout_runtime::World<'static> =
    fallout_runtime::World::new(source, fallout_runtime::Limits::default())?;
```

World instances keep separate locals, references, pending events, clocks, item
banks and transient handle epochs even when their source pointer is identical.
Snapshot and native-save bytes do not encode ownership mode or Arc addresses.
Restoration continues to check the whole source cohort and exact script versions.
An invalid replacement preserves the existing world. Successful replacement
creates a new handle epoch and drops its old source owner after validation.

Repository fallback clones the source owner before trying the current slot, so
an invalid current save cannot consume the source needed to restore the previous
slot. Recovery remains explicit and retains the existing repair policy. Failed
attempts release their temporary Arc owners. Borrowed and owned callers use the
same container and schema version; no migration is required.

Eight runtime tests cover lifetime after loader destruction, Send/Sync worker
transfer, byte-identical borrowed/shared snapshots and saves, independent mutable
worlds, restored handle rejection, atomic replacement, repository fallback/repair
and full source-cohort checks. Weak owners verify release after the last world is
dropped. These are engine guarantees, not measured original behavior.

The `shared-runtime` CLI loads the supplied source order into one shared owner,
seeds the existing explicit engineering state, drops its loader/store, moves an
owned world to a worker and checks every winning source handle and persistent
instance identity. It also checks canonical state bytes, replacement atomicity
and release of the last source owner. A fresh cold/warm comparison joins both
runs to the independently checked source catalogue and existing schema/state
report. The schema reader establishes original declarations; the synthetic state
values do not claim retail initialization or numeric conversion.

```powershell
.\target\release\fallout.exe shared-runtime --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles\nv-inspection-order.json --output local\YOUR-NEW-REPORT.json
```

Source payloads stay local. The public receipt records hashes, counts and check
results. This checkpoint supplies ownership needed for runtime integration;
execution frames, actor/player initialization, original scheduling and gameplay
acceptance remain unfinished.
