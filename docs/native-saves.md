# Native script-state saves

The runtime can capture an owned snapshot at a host boundary and publish it from
a worker while the host continues changing its world. Loading rebuilds canonical
state against the same source-bound catalogue. This checkpoint persists script
instances, local banks, references, contexts, clocks and pending events. Player,
actor, quest and complete world state still need their own components.
[Explicit item banks](item-runtime-state.md) are included beginning with schema 3.
[Explicit reference pose and enable state](reference-state.md) are included in
schema 4, with unavailable components preserved by explicit legacy imports.

```powershell
.\target\release\fallout.exe native-save-probe `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --new-repository local/native-save-example `
  --output local/native-save-example.json

.\target\release\fallout.exe native-load-probe `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --repository local/native-save-example `
  --output local/native-load-example.json
```

The save probe requires a new directory. It writes explicit engineering inputs,
saves two states, truncates only its own new current slot, verifies explicit
previous-slot recovery, repairs it, then republishes the later state. It never
captures original live values or opens Bethesda `.fos` files. Output paths must
also be new. `native-save-file --file <path>` validates a container without
claiming source-bound restoration.

## Identity and capture

A nonzero 128-bit campaign identity scopes persistent instance and reference IDs.
Normal world creation obtains OS random bytes through pinned `getrandom` 0.4.3;
failures propagate. Imports and deterministic tests can provide an explicit
identity. Every successful canonical mutation increments a checked 64-bit state
revision. Revision exhaustion rejects the operation before changing state.

`Captured::at_boundary` owns a complete snapshot. Publication never reads later
world mutations. A repository rejects other campaigns, changed content cohorts,
older revisions, backward clocks/allocators and different states at the same
revision. Identical repeated captures are allowed. A writer lock serializes
publication across processes; contention returns a busy error.

## Background writer

`SaveWorker` publishes accepted captures in submission order on one named thread.
Its capacity includes the snapshot being written and every queued snapshot. Choose
between one and 64 outstanding requests; two is a reasonable starting point for
the host. `SaveWorker::start` also applies a 256 MiB aggregate snapshot reservation
budget. `SaveWorker::start_with_budget(repository, count, bytes)` supplies an
explicit positive budget. Admission reserves each capture's declared
`max_snapshot_bytes`, with one synchronized count/byte decision. Default 64 MiB
snapshot limits therefore permit at most four simultaneous captures even when
the configured request count is larger. No admission serialization is required.

The charge is a conservative serialized snapshot upper bound, not exact heap
usage or a total process memory ceiling. Capture itself copies canonical state at
the host boundary. Each capture keeps its runtime limits, and encoding still
enforces that snapshot byte limit on the worker. Choose an aggregate budget that
admits the snapshot limits used when creating or restoring the host world.

```rust,ignore
let mut saves = SaveWorker::start(repository, 2)?;
let capture = Captured::at_boundary(&world);
let mut ticket = match saves.try_submit(capture) {
    Ok(ticket) => ticket,
    Err(rejected) => {
        // Keep rejected.capture if this exact boundary should be retried.
        // Admission is not a write receipt; report the rejection to the host.
        return Err(rejected.into());
    }
};

// Poll on later frames. None means that filesystem work is still pending.
if let Some(receipt) = ticket.try_wait()? {
    // A receipt confirms this request's publication, including its generation.
    report_save(receipt);
}
```

Keep each ticket until its terminal success or error has been handled. Results
are delivered once; polling a consumed ticket reports `AlreadyCollected`.
Count or byte saturation, or a stopped worker, returns the original capture to
the caller. Oversized declared limits reject before publication. Active and
queued jobs retain their reservations until their owned storage is released;
completion, writer errors, panic, disconnect and draining shutdown release each
reservation exactly once. Dropping a result ticket does not cancel an accepted
save or release its reservation early. A short mutex protects only the admission
counters; filesystem work and result delivery occur outside that critical section.
Accepted requests are neither combined nor automatically retried. A busy
repository, stale revision or disk failure is returned on that request's ticket;
the writer continues to the next request. A stopped writer reports an explicit
completion error rather than claiming that the state was saved.

Call `finish` outside the frame loop to close admission, drain accepted work and
join the writer. Successful shutdown confirms draining and joining; individual
write results still come from their tickets. Tickets remain usable afterward. Dropping a ticket does not
cancel its write. Dropping the worker also drains and joins, so early returns
cannot detach a writer that is still changing save slots. This can wait on a
slow filesystem; explicit shutdown provides the worker-panic result. Process
termination and filesystem recovery retain the separate repository guarantees
described below.

The installed-content `native-save-probe` uses this writer without changing its
report or container schemas. Seven additional worker tests cover active/queued
saturation, exact returned captures, FIFO generations, dropped tickets, drain on
drop, worker panic, consumed results, independent world/catalogue lifetimes,
script/reference/item/event restoration and request-error recovery. These are
native host tests, not measurements of Bethesda quicksave timing or behavior.

## Container contract

All integers use little endian. `FRSAVE01` has a 16-byte header: eight magic bytes,
container version `u16 = 1`, reserved `u16 = 0`, and chunk count `u32 = 2`.
Each chunk begins with a four-byte tag, `u32` version, `u64` payload length and
32-byte SHA-256. A final 32-byte SHA-256 covers all preceding container bytes.
Unknown versions, tags, reserved values, trailing bytes and invalid extents fail.

| Chunk | Version | Payload |
| --- | --- | --- |
| META | 1 | 88 bytes: NV adapter ID, state schema, generation, boundary tick, cohort digest, snapshot length, campaign identity and state revision |
| STAT | 1 | Exact canonical snapshot JSON, schema 4 |

The fixed overhead is 232 bytes. The default snapshot limit is 64 MiB; filesystem
length is checked before allocation. META must agree with STAT's identity,
revision, clocks, schema, length and cohort. Restoring also validates complete
local schemas, content versions, all persistent links and pending event order.

Current snapshot decoding and explicit schema-1/2 migration first traverse the
bounded JSON input without constructing owned state or collection vectors. The
pass counts references, script instances, pending events, aggregate locals and
items, inventory banks, per-context arguments, and per-item/aggregate links and
opaque bytes. Optional null item facts contribute no links. Escaped field names
and positional struct arrays use the same limits as ordinary object fields.
Every nested value is traversed with a recursion bound, including malformed or
irrelevant fields; `IgnoredAny` is not used for this admission pass. JSON may use
string scratch space up to the input byte limit. This is collection admission,
not a complete heap ceiling.

The existing strict DTO decoder and restoration validation still run afterward,
so unknown/duplicate fields, invalid values, identity/link errors and unsupported
schemas remain rejected. Oversized collections return the existing capacity
errors before owned deserialization. Snapshot and native-envelope schemas and
explicit migration semantics are unchanged. `snapshot_admission` tests cover
early rejection, aggregate and exact limits, escaped names, positional current
and legacy forms, malformed/deep input and source-bound restoration.

Checkpoint 28's in-memory schema 1 can be migrated explicitly with
`Snapshot::migrate_v1`, supplying a campaign identity. It preserves supplied
values and assigns revision zero to the new bookkeeping field. Schema 2 raw snapshots
can migrate explicitly with `Snapshot::migrate_v2`, preserving campaign and revision.
Both migrations leave inventory banks uninitialized and start the item allocator
at one; they do not invent original inventory contents. Normal decoding
does not silently migrate old state. The migrated snapshot still needs full
source-bound restoration. Normal native-envelope loading rejects old state schemas.
[Explicit native import](native-save-migration.md) now handles schema 2 envelopes
in a new repository after full validation. Schema 3 uses the explicit
`native-migrate-v3` command or `save::format::migrate_v3`, preserving inventory and
leaving reference pose/enable unavailable. Original-save compatibility is separate.

## Publication and recovery

Repositories use a native campaign marker and fixed `current.frsv` and
`previous.frsv` slots. Creation never adopts an existing folder. Canonical paths
must lie outside protected inputs; repository files and directories reject
symlinks and Windows reparse points. This protects normal local operation, not a
hostile process concurrently replacing directory ancestry.

Under the writer lock, publication validates the current slot, writes and syncs
a uniquely owned temporary, publishes a synced copy of the valid current state
to the previous slot, then replaces current in the same directory. An unpublished
temporary is removed on ordinary failure. Interrupted processes can leave owned
temporaries; those are not treated as valid save slots. A blocked backup write
leaves current intact. Unknown or corrupt current state prevents a new commit.

Publication checks intrinsic snapshot identities and links for both the proposed
capture and the decoded current state before preparing any temporary or rotating
previous. Restoration shares those checks: allocator bounds and unique identities,
owners, local/context references, item/script links and pending-event order/clocks.
A checksummed current snapshot with a dangling link therefore cannot overwrite
a valid previous save. Rejection preserves both slots; recovery and repair remain
explicit, followed by a fresh retry. The public APIs and native wire bytes are
unchanged. Publication has no loaded catalogue: definition versions, complete
local declaration kinds and compiled event sites still need source-bound loading.

Strict loading is the default. Explicit fallback returns the restored previous
state and the current failure without rewriting current. Explicit repair first
fully restores previous against the catalogue, then copies it to current. Neither
path drops unsupported state or conceals recovery in a successful current load.

`native-save-probe --engineering-event-commit FILE` reuses the bounded explicit
[script-state transaction request](script-runtime-state.md). It validates and
commits the engineering request before creating the new repository or starting
its writer; rejected requests publish no save. The inherited save worker then
stores captures from before and after that transaction. The probe checks both
complete snapshots, previous-slot fallback and explicit repair, and republishes
the committed snapshot for a separate cold `native-load-probe` process. Its
optional report includes the transaction receipt, exact snapshot hashes and the
strict current-slot failure. The default probe and report remain unchanged.

Staged save-worker tests retain full script, event, reference and inventory state
after the originating world and catalogue are dropped. Corrupt-current fallback
restores the pending precommit event; repair permits freshly staging that event
again in the restored world. Separate publication interruption tests terminate
the repository writer at all five observed stages, then cold processes verify a
complete pending or committed boundary and an actual restarted save worker
publishes it again. The interruption observer exercises the existing repository
publication path used by the save worker. It does not add a second journal or
change the save format.

The existing staged interruption test also compares complete current and previous
container bytes at every observed stage, before restarting the save worker.
Previous is absent until its publication; afterward, its bytes match the complete
precommit capture. Separate cold children verify explicit previous-slot fallback
after truncating a copied current slot, leaving both copied slots unchanged.
Set `FALLOUT_STAGED_INTERRUPTION_EVIDENCE` to a fresh existing private directory
to retain authored inputs, interrupted slot copies, restarted worker slots and
per-stage metadata for independent native-reader and cold CLI comparisons.
Normal test runs use an automatically removed temporary directory.

Repeated transaction tests retain proposals across actual native loads and an
equal snapshot replacement. The earlier world epoch rejects those proposals even
when every persisted field matches. Duplicate proposals and acknowledged-head
requests also reject before changing the restored world or save slots. Two fresh
head commits each advance revision once; explicit previous recovery can stage
the remaining head again and reproduce identical current/previous bytes.
Set `FALLOUT_STAGED_RESTORE_EVIDENCE` to a fresh existing private directory to
retain the three exact captures and rejection receipts for cold native readers.

File contents are synced using [Rust File::sync_all](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all),
and publication uses [same-directory rename](https://doc.rust-lang.org/std/fs/fn.rename.html).
The [nonblocking file lock](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
is released when its owning handle closes. Real child-process termination tests
cover five write, sync and publication stages on this Windows NTFS installation.
They verify complete old/new slots and released locks. Rust's safe standard API
does not provide a Windows directory flush here; power-loss durability and remote
filesystem behavior have not been verified. These tests establish process
interruption recovery only.

## Evidence

Eleven save tests cover worker isolation, cold process loading, campaign/revision
conflicts, malformed metadata and snapshots, protected folders, blocked backup
publication, explicit recovery, migration and killed writers. Eleven existing
state tests also remain applicable.

The historical checkpoint 29 schema 2 installed-content probe preserves 2,483 instances and 2,160 pending events.
The later canonical snapshot is 2,381,546 bytes, with SHA-256
`f0655cb269a06daf8106fe408c7b4d0be167b0904e9c0952cc56bee6df556ed0`.
Its values are explicit test inputs. A fresh process obtains identical bytes.
An original offline C++ reader independently validates both container slots,
metadata, extents and hashes; it does not parse canonical JSON semantics. The
evidence driver also reruns all original compiled-schema comparisons and rejects
27 malformed or oversized container cases in both readers.

No original gameplay or save compatibility is accepted. Native saves, Bethesda
save import, NVSE cosaves and third-party DLL state remain separate tracks.
The selected `getrandom` manifest, dual MIT/Apache notices, API and Windows backend
were read. The dependency is linked unchanged for campaign bytes; no RNG or
upstream persistence implementation was copied. Its
[fill API](https://docs.rs/getrandom/0.4.3/getrandom/fn.fill.html) propagates failure;
the reviewed Windows backend uses `ProcessPrng`.
