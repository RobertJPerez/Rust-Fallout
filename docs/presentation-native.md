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
