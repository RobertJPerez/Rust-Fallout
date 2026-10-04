# Explicit canonical reference state

Runtime owns persistent `ReferenceId` and the source `FormKey` already registered
in `World`. Cell residency, physics and presentation consume
`World::reference_view(id)`. The owned immutable `reference_state::View` identifies
campaign, source cohort, global revision, reference, optional authored origin and
optional state. Dropping source resources or render entities does not unregister
the persistent identity. On reload, resolve `World::authored_reference(key)` and
read the retained state; initialize from verified source inputs only when state
is unavailable. Registering a reference does not fabricate pose or enable state.

`Pose::from_source(&fallout_data::world::Transform, Option<f32>)` retains exact
finite f32 position and rotation bits, including signed zero and subnormals.
Absent XSCL remains `None`; explicit scale must be finite and positive, matching
the existing placement decoder. `source_transform()` and `source_scale()` supply
the existing `fallout_data::coordinates` adapter. No axis remapping, cell offset,
rotation composition, retail-unit conversion or scale default is introduced.

`State::new(cell, pose, enabled)` requires a canonical NV source cell key and an
explicit boolean. The cell identifies residency; the position remains in original
DATA coordinates. Callers bind cell and authored reference keys through the
existing source record consumers. Runtime checks key syntax, cohort, identity,
finite storage and revision. It does not read source record bodies, establish
cell membership or evaluate source enable parents. No source lookup semantics,
player spawn, actor defaults or retail enable behavior is claimed by this API.

Stage with `World::stage_reference_state(&view, state)` and commit by consuming
the resulting opaque `Staged` through `World::commit_reference_state(stage)`.
Staging and dropping have no effect. A successful commit increments the global
revision once and returns `Receipt` with source/campaign identity and explicit
committed state. Any intervening canonical mutation invalidates the view/stage,
including mutations to other components. A different or restored World has a
fresh transient epoch, preventing equal numeric revisions from admitting old
intent. Views and stages cannot be deserialized as mutation authority; the view
epoch does not serialize. State remains private to the runtime bank.

Snapshot schema 4 adds `reference_states`, a bounded collection of
`{id, state}` rows. Each row requires an existing reference identity, is unique,
and contains component `schema_version: 1`, `cell`, `pose` and `enabled`. Pose
contains fixed `position_bits` and `rotation_bits` arrays and optional
`scale_bits`. Invalid component versions, non-finite bits, invalid explicit
scale, duplicate rows and dangling IDs refuse before state replacement or save
rotation. The collection admission pass charges each row against
`Limits::max_references` before constructing owned state, including positional
and escaped-field JSON. This is an independent upper bound on component rows;
intrinsic relationship validation then checks their reference links.

Explicit `Snapshot::migrate_v3` and `save::format::migrate_v3` retain existing
inventory, script state, clocks, IDs, source cohort, campaign and revision. They
leave the new bank empty. Existing explicit v1/v2 migrations also leave it empty;
their previous campaign/revision/inventory rules remain. Legacy DTOs reject new
component fields, duplicate and unknown fields rather than interpreting them as
live state. Normal load stays strict; `fallout-cli native-migrate-v3` imports into
a new repository, retaining the write-denying source handle through validation
and publication. Existing native schemas require explicit import; no automatic
fallback or in-place rewrite is added.

The headless source-selected cell consumer in `tests/reference_state.rs` decodes
authored CELL/REFR records through the existing decoder, supplies explicit enable
input, reuses identity after unload and reload, publishes through the real worker,
and restores exact pose/enable and script links in a fresh child process. This
producer verification does not establish a completed preview consumer or retail
gameplay equivalence. Presentation binds the tested producer under REF-STATE;
runtime never stores render entities as authority.

`reference_state_transactions` exercises the real prepared source-local-copy
engineering consumer against a reference-state stage at the same global revision.
Both winner orders are checked against the complete canonical snapshot; the loser
cannot return a success receipt, write a local, acknowledge an event or alter
reference state, inventory or clocks. Inventory and clock changes invalidate
both kinds of stage. Restore, other campaigns and changed whole source cohorts
also refuse old authority. Winner snapshots pass existing native publication and
source-bound restoration. Original assignment conversion and callback behavior
remain unverified; these are explicit engineering transactions.

`World::reference_state_page(PageRequest { cell, after }, PageLimits)` supplies
bounded immutable scene observations in stable `ReferenceId` order. `cell: None`
includes the whole registered population, including unavailable components;
`Some(cell)` matches only an explicitly stored component naming that exact key.
The page exposes campaign, cohort, revision, filter, existing `View` rows,
`PageUsage` and an optional opaque continuation. `into_parts()` transfers rows
and cursor to the consumer. It traverses the registry directly without making a
whole-world snapshot. Source cell membership and source defaults remain caller
inputs, as in the single-reference API.

`max_visited` bounds inspected registry entries independently of matches, and
`max_rows` bounds returned rows. Both must be positive. A zero-match page can
still advance its cursor. `max_copied_bytes` admits a conservative charge for
fixed owned page/view values and their UTF-8 strings before copying them,
including reserved continuation metadata when entries remain at page start.
This charge excludes allocator overhead and spare vector capacity; it is not a
process memory ceiling. A matching row that cannot fit remains unconsumed for
the next page. If the first candidate cannot fit, the call refuses instead of
returning a continuation without progress. Limits do not cause proportional
preallocation; empty registries return a complete empty page.

The cursor binds its producing World epoch, campaign, cohort, global revision,
exact filter and last consumed reference. Mutation, equal-state restore, other
Worlds and changed filters refuse before copy admission. Cursor fields are
private and cannot deserialize; cursors are never saved or used as render IDs.
`tests/reference_state_paging.rs` runs a real headless multi-cell reconciliation
consumer, verifies zero-match progress and byte-limited coverage, then publishes
changed pose/enable through the native worker and reconciles exact retained
state in a fresh child process. This adds an immediately runnable producer
consumer; completion of the presentation lane's preview remains separate.
