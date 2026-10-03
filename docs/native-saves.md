# Native script-state saves

The runtime can capture an owned snapshot at a host boundary and publish it from
a worker while the host continues changing its world. Loading rebuilds canonical
state against the same source-bound catalogue. This checkpoint persists script
instances, local banks, references, contexts, clocks and pending events. Player,
inventory, actor, quest and complete world state still need their own components.

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

## Container contract

All integers use little endian. `FRSAVE01` has a 16-byte header: eight magic bytes,
container version `u16 = 1`, reserved `u16 = 0`, and chunk count `u32 = 2`.
Each chunk begins with a four-byte tag, `u32` version, `u64` payload length and
32-byte SHA-256. A final 32-byte SHA-256 covers all preceding container bytes.
Unknown versions, tags, reserved values, trailing bytes and invalid extents fail.

| Chunk | Version | Payload |
| --- | --- | --- |
| META | 1 | 88 bytes: NV adapter ID, state schema, generation, boundary tick, cohort digest, snapshot length, campaign identity and state revision |
| STAT | 1 | Exact canonical snapshot JSON, schema 2 |

The fixed overhead is 232 bytes. The default snapshot limit is 64 MiB; filesystem
length is checked before allocation. META must agree with STAT's identity,
revision, clocks, schema, length and cohort. Restoring also validates complete
local schemas, content versions, all persistent links and pending event order.

Checkpoint 28's in-memory schema 1 can be migrated explicitly with
`Snapshot::migrate_v1`, supplying a campaign identity. It preserves gameplay
values and assigns revision zero to the new bookkeeping field. Normal decoding
does not silently migrate old state. The migrated snapshot still needs full
source-bound restoration. There was no earlier native filesystem container to
import; this migration does not provide original-save compatibility.

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

Strict loading is the default. Explicit fallback returns the restored previous
state and the current failure without rewriting current. Explicit repair first
fully restores previous against the catalogue, then copies it to current. Neither
path drops unsupported state or conceals recovery in a successful current load.

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

The installed-content probe preserves 2,483 instances and 2,160 pending events.
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
