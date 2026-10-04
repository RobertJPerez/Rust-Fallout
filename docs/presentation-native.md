# Canonical reference display and native save host

The CELL inspector accepts `--native-save REPOSITORY` with its existing explicit
installation, load order and CELL. The repository must already carry the project's
native marker and a current schema-4 save. Restoration uses the runtime's strict
source-bound load against the same record store used to inspect the CELL. There
is no automatic migration, previous-slot recovery, new campaign, reference
registration, source initialization or original Bethesda save import.

Canonical observations retain campaign, content cohort, revision, authored
`FormKey`, persistent `ReferenceId` and exact source float bits. The ECS entity is
only a transient view. Selected source models and their local geometry still
come from the existing asset adapters; canonical pose supplies the placement
once through `coordinates::Affine::nv_reference` and `relative_view`. The source
origin is subtracted in f64 before narrowing to a finite Bevy transform.

Missing canonical identity, missing component, absent scale, disabled state and
a different current CELL are distinct report outcomes. Those views remain
hidden; an unavailable scale never becomes a canonical value of one. Retained
source inspection bounds can still frame hidden resources. Deleted source
references, unsupported actors and unavailable model assets remain omitted with
their existing diagnostics. This is an explicit engineering view of canonical
state, without original enable-parent evaluation or gameplay initialization.

F5 submits one real runtime capture and F9 reloads the strict current repository.
These inspection bindings use the same fresh keyboard transitions and focus/
context quarantine as camera shortcuts. Loading and suspended contexts suppress
both actions. One request is admitted at a time; additional presses do not create
an unbounded queue or an implicit retry.

Keyboard admission emits one typed intent with its action, monotonic sequence,
primary window, focus/context, device and the displayed scene generation/revision.
The display identity is available only after native draw publication and changes
after a complete Continue view update. The main adapter rechecks its current
window focus/context; the native host consumes each sequence once and rejects a
different scene/revision. A busy host consumes an additional sequence without
queuing it, so completion cannot replay a held or refused action. Sequence
exhaustion emits no command. Controller native-save bindings are unassigned;
existing controller camera/close boundaries remain separate inspection actions.

The owner command carries the expected revision and checks it against the owned
canonical world before capture or restore. An input sampled from the previous
display cannot save a newer world if Continue completes before admission. These
checks are private host transport; the presentation thread neither mutates
canonical state nor supplies guessed reference values. The explicit headless
save-after-ready request uses the same owner precondition without a physical key.

The native owner keeps the shared source catalogue and canonical world on its
own thread. Capture creation, source-bound restore, disk publication and writer
shutdown stay outside the frame loop. `SaveStatus` observes the existing
`SaveTicket`; admission remains pending until its actual publication receipt or
failure arrives. The window title retains the terminal result. Receipt generation
and revision are logged directly from `WriteReceipt`. Joining a writer is never
reported as save success. Continue failures keep the prior active boundary.

Window close disconnects request admission. After `App::run` returns, an owned
shutdown registry closes late loader admission and drains native owners outside
render/input updates. This preserves accepted writes without holding a frame on
filesystem work. The registry admits at most eight outstanding native owners.
Later source preparation joins only owners that have already returned, reclaiming
their slots so sequential retries do not exhaust a lifetime counter. Active owners
are never joined in render/input updates. A collected panic remains a sticky
failure for later admission and final shutdown; collection is never a save receipt.

`--native-save-after-ready` is an explicit headless engineering switch. It admits
one save only after complete draw publication, waits for the real result before
capture, and exits with failure on a failed save. The initial CELL JSON report
describes its strict restore and read-only observations, not the subsequent save.
Original installations, original saves/settings and raw source material are
protected inputs. Native writes require the explicitly selected project-native
repository outside the installation.

This consumer proves engineering restoration/display and publication behavior.
It does not establish original gameplay, visual parity, Continue-menu parity,
power-loss durability or compatibility with original save files.

`--native-edit REQUEST` is an opt-in engineering request for one exact current
displayed source reference. The strict JSON object supplies schema version 1,
nonzero scene epoch and intent sequence, expected campaign/cohort/revision,
authored `FormKey`, persistent reference ID and a complete component `State`.
Every position/rotation float and the explicitly present scale field retains
its source binary32 words; scale can be null for a disabled result. There are
no inferred pose, enable, CELL or physical keyboard/controller movement rules.
Input is limited to 4 KiB. Unknown fields, absent component fields, nonfinite
words and invalid scales are refused. The existing canonical CELL stays fixed.

The host adapter requires exactly one matching current `ReferenceView`. The
existing sole native owner checks its current source/campaign/revision, obtains
a fresh runtime view and uses `stage_reference_state` and
`commit_reference_state`. It validates the entire prior observation, proposed
finite renderer placement and bounded receipt before that commit. A consumed
sequence is never replayed; the existing one-request host channel limits edits,
Save and Continue together. An edited source pose passes through the existing
source basis/origin conversion exactly once. The immutable postcommit
observation updates current displayed identities, placement and visibility;
an old epoch/revision/result cannot move the current draw.

Edit acceptance does not publish a save. Its receipt includes the actual
canonical commit and before/after source views, with durable publication false.
F5 uses the existing writer, and F9 uses strict source-bound Continue. With
`--native-save-after-ready`, an opted-in edit must first be committed and applied
to the display before Save is submitted. Failed edit or Save produces failure,
retains the appropriate prior boundary, and cannot produce a successful capture.

Capturing an edited display requires a separate fresh `--native-edit-receipt`
path. The final receipt retains the edit commit, displayed revision, actual Save
publication when requested, and actual PNG path/hash/bytes/dimensions. Its 1 MiB
ceiling includes the newline; PNG hashing uses the existing 8 MiB bound. The
initial CELL report remains an immutable record of initial restoration. Output
paths are distinct and stay outside the installation, edit-input directory and
selected native repository. This demonstrates caller-authored canonical editing
and persistence, without collision, movement simulation or retail gameplay proof.

`--native-inventory REQUEST` queries one already displayed complete canonical
owner through the same native owner channel. Its strict 4 KiB JSON request binds
schema version 1, nonzero scene epoch/sequence, expected campaign/cohort/revision,
authored key, persistent owner ID, three explicit producer limits and output
bytes. Limits can only be lowered from 128 lots, 1024 Facts links and 64 KiB of
opaque extra payload; the complete encoded observation including newline is
limited separately to 1 MiB. These are the existing producer's three logical
copy limits. They do not claim a total allocator or fourth producer byte ceiling.

The owner calls the reviewed `World::inventory_view` without cloning the World,
initializing a bank or changing any item. Source bit patterns, opaque payload,
lot IDs and unequal counts remain intact. Exactly one owned observation is
retained; it becomes usable after current display admission and is dropped for
an edit, Continue or scene replacement. Unavailable, initialized empty and a
retained lot count are distinct bounded window-title outcomes. A stale authority
epoch, owner, revision or refused limit returns no usable partial observation.

The optional fresh `--native-inventory-report` accompanies an actual capture and
contains the complete existing `InventoryView` and source/display identity.
Output never writes into the source request directory, installation or native
repository. In the combined engineering request, querying waits for the explicit
edit to reach the display and for a requested Save to publish; its revision must
be supplied for that exact boundary. These are separate runtime operations:
a later rejected query does not roll back an already accepted edit or Save.
No original inventory layout, font, sorting, stack merge or equip behavior is
inferred. Close and SaveStatus polling remain outside query/encoding work on the
native owner thread.
