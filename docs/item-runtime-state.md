# Canonical item state

`fallout-runtime` owns explicit mutable item banks independently of rendering.
An initialized empty bank differs from a bank whose original contents are still
unknown. Host calls add, split, transfer and remove quantities, or replace an
item's facts. Validation and checked arithmetic finish before a mutation changes
state. Each successful change advances the canonical revision.

Persistent nonzero item IDs have a separate monotonic allocator and belong to the
world's campaign. Splitting creates a new ID; transferring preserves the ID and
all facts. Removed IDs are not reused within that timeline. Transient item handles
expire on removal or world restoration and are never serialized. Equal stacks
remain separate, including stacks whose facts are unknown.

Facts preserve an explicit base key, optional condition, ownership, equipment
slots, ammunition, modifications, quest-item status and script instance, plus
ordered opaque fields. An absent optional fact means unknown. It does not mean a
default, unequipped, unowned or false. Condition storage distinguishes exact
32-bit and 64-bit patterns, including signed zero and NaN payloads. No conversion
or clamping is applied. Quantity is a positive host `u32`; this does not establish
conversion from the original signed source counts or count deltas.

Live ownership and script links must exist. An item script link prevents removal
of that instance until explicitly detached. Content keys currently receive
canonical syntax validation; header existence, kind and original admission rules
are separate unfinished work. Ammunition count, equipment slot IDs and modification
keys are explicit host fields, not a claimed decoding of original extra-data
words. Transfers do not silently change these facts or run callbacks.

## Queries and persistence

An ordered owner index enumerates only that bank. A derived count index answers
an exact host quantity sum in `O(log N)`, using checked `u64` arithmetic. Count
traces include campaign, revision, clocks, subject, base key and each contributing
item ID/quantity. Trace allocation has an explicit contribution budget. This is
an engineering query; original `GetItemCount`, condition adapters and numeric
return coercion remain unimplemented.

Snapshot schema 3 saves canonical banks, item identities, facts and allocator.
Restore validates every link and budget in a separate world, then rebuilds both
derived indices. Neither indices nor transient handles enter the save. Explicit
raw-snapshot migration from schema 2 preserves campaign, revision and prior state;
schema 1 also requires a supplied campaign. Both leave inventories uninitialized
and start the item allocator at one. [Native envelopes have a separate explicit import](native-save-migration.md);
normal loading never silently converts unsupported schemas.

```powershell
.\target\release\fallout.exe item-state `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --new-repository local/item-state-example `
  --output local/item-state-example.json
```

The probe uses three independently checked original base-inventory identities
with disclosed test quantities and attributes. It exercises split, transfer,
removal, fact replacement, rejected mutations and an owned worker capture. A
separate `item-load-probe` process loads that repository and repeats the exact
queries. Its strict `--query-inputs` JSON has `owners` and `item_keys` arrays, each
containing 1–16 values; the file limit is 64 KiB and the batch contribution limit
is 65,536. Output and repository paths must be new and outside the installation.

Fourteen runtime tests include a 2,000-operation sequence checked after every
step against canonical snapshot sums, bounded allocation, exact facts, stale
handles, migration and native restoration. The independent C++ save reader
checks container metadata and integrity; it does not validate JSON semantics.
Source identity agreement is independently checked against original plugin
fields. Neither comparison accepts original inventory behavior.

Selected pinned [xNVSE extra-data declarations](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameExtraData.h)
were read at lines 303–318, 546–574, 599–610 and 681–735. They expose script,
health, worn, count, ammo, ownership/rank/global and modification storage. Opaque
constructors and unknown words do not establish behavior. No upstream
implementation was copied or linked; exact scopes and hashes are in the source lock.

Original initialization, inventory deltas, leveled expansion, stacking,
equipment/ownership enforcement, ammunition/modification semantics, lifecycle
callbacks and complete world persistence remain open. No retail scenario is
accepted by this checkpoint.
