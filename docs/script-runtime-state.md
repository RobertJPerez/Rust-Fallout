# Script instances and canonical state

`fallout-runtime` now separates immutable script definitions from mutable
instances. It has no Bevy dependency. A running host can store exact local values,
typed references, ownership and pending event context, then restore that state
against the same content cohort. Bytecode execution and retail scheduling are
still unfinished.

```powershell
.\target\release\fallout.exe script-state `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --output local/script-state.json
```

The command independently inspectable output covers 80,627 winning script units,
including units without SCTX. There are 11,172 unique local indices: 1,077 numeric
float declarations, 8,067 integer declarations and 2,028 reference declarations.
All eleven duplicate indices keep their first declaration. The three previously
documented stale SCHR counts remain findings in the loaded catalogue.

The engineering probe instantiates the 2,483 definitions with locals using
explicit fragment identities and synthetic values. It round-trips 9,144 numeric
values, 2,028 reference values and 2,160 pending events. Checkpoint 28's schema 1
snapshot was 2,381,445 bytes; schema 2 adds campaign identity and state revision.
Those values are test inputs, not observations of original live state.

## Identity and storage

Each world has a persistent nonzero 128-bit campaign identity. Persistent script
and reference IDs use separate monotonic nonzero 64-bit
allocators. Removed instance IDs are not recycled. Transient handles use a world
epoch, slot and generation; removal, slot reuse and restoration invalidate old
handles. Saves contain persistent identities, never raw pointers or ECS entities.

Numeric slots hold all 64 bits of double-width storage. Signed zero, infinities,
NaN payloads and integer-valued doubles retain their exact bits. Reference slots
store null, a namespaced content key, or a persistent live-reference ID; they do
not encode identities through a float. This is storage evidence, not acceptance
of original arithmetic, integer coercion or native return behavior.

Compiled SLSD declarations supply numeric kinds. Positive SCRV membership
identifies reference locals. Index zero has an explicit unverified classification
because it overlaps the reviewed static-reference sentinel; the original corpus
contains no declaration at index zero. Unknown types remain present and
uninitialized. Neither missing values nor constructor defaults silently become
zero. An explicit reset zeros supported numeric storage and nulls references;
it rejects unverified types before changing any slot.

Assignment batches validate every index, kind and reference before mutation.
Duplicate assignments are rejected. Instance ownership and contexts are explicit
host inputs. Quest attachment metadata alone does not construct a running event
list, and a base-object script is not substituted for a placed reference's list.

## Events and snapshots

Compiled BEGIN descriptor IDs and object-event bit masks are separate types.
Block events validate both the authored byte offset and descriptor ID. Object
mask observations preserve unknown bits without guessing a block mapping.
Declaration schemas and event-block lookups are cached per definition and shared
between instances. Lookup uses ordered indices instead of rescanning bytecode
for each event.

The pending journal preserves arrival order, caller, containing reference,
target, arguments and host-supplied game/menu/real clocks. FIFO is the native
journal policy; original dispatch order, delay, suspension and pause behavior
remain unmeasured. Acknowledgment is explicit and does not execute bytecode.
Pending events prevent implicit instance removal.

Schema version 2 snapshots retain campaign identity, state revision, allocators,
references, instances, local banks, clocks and pending events. Explicit migration
from schema 1 requires a campaign identity. A content fingerprint binds normalized source names,
lengths and SHA-256 values, the digest of every winning content header, winning
script versions and resolved reference
provenance. Reordering the same sources without changing winners preserves it.
Changing source bytes or relevant winners invalidates restoration.

Restoration builds a separate world, validates complete local schemas and every
identity/context/event, then replaces the caller's state. Unknown JSON fields,
unsupported versions, duplicate identities, missing declarations, wrong storage
kinds, dangling references, invalid event order and allocator reuse are rejected.
Collection and byte budgets are explicit. This layer provides bounded canonical
JSON encoding; [native saves](native-saves.md) add filesystem publication and
explicit previous-slot recovery.
It does not read or write Bethesda `.fos` files.

## Evidence and references

The original offline C++ schema reader opens original plugin sources with retained
write-denying handles, rebuilds whole-record winners and directly extracts bodies.
Compressed bodies use the independently tested checkpoint 26 decoder. Every
source extent, decoded digest, unit marker and first local classification agrees
with Rust. Coverage is also checked against the bound checkpoint 24 catalogue.
Authored fixtures cover sparse, duplicate, reference, unknown, zero and full-width
indices, missing SCRV declarations, source reordering and cold/warm cache reuse.
Ten malformed source cases fail without publishing a report.

Eleven runtime tests cover atomic updates/reset, exact bit patterns, stale handles,
reference bindings, contexts/clocks, budgets, changed content and malformed
restoration. Runtime storage and snapshot invariants are engineering tests;
schema agreement is an independent format comparison. No gameplay scenario is
accepted by either kind of evidence.

The selected pinned xNVSE sources reviewed for this checkpoint are
[GameScript.cpp](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameScript.cpp)
lines 175â€“191 and 323â€“350 (compiled variable classification and reference
resolution/opaque constructor),
[GameAPI.h](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameAPI.h)
lines 157â€“238 (storage, event masks and list structure), and
[GameAPI.cpp](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameAPI.cpp)
lines 919â€“997, 1097â€“1127, 1917â€“2001 and 2360â€“2365 (copy, reference extraction,
reset, first lookup and external event-list selection). Selected GameScript.h
lines 1â€“150 describe execution context and type declarations. No implementation
was copied. These sources expose storage operations; they do not expose or prove
original constructor values or event scheduling.

Next: foreign live-variable context,
primitive queries and observable execution traces. Inventory, quest/dialogue
mutation and complete world snapshots remain future work.
