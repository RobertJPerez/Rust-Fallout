# Base inventory declarations

Winning `CONT`, `NPC_` and `CREA` records now have a bounded, immutable inventory
catalogue. It retains the original decoded body and every field's position, length
and hash. Unknown fields remain accessible through the retained record. Whole
source hashes and canonical master-relative keys identify the exact input cohort.

`CNTO` contains an item FormID and signed 32-bit count. Duplicate entries, negative
counts and zero counts remain ordered. Optional following `COED` fields retain
their owner, union word and exact condition bits. Multiple extras and orphan extras
produce source findings instead of choosing or discarding an owner. Field offsets
identify decoded field headers; extended `XXXX` framing remains in the retained body.

For a defined NPC owner, the union word is a global FormID; for a defined faction it
is a signed required rank. A null owner leaves an unused raw word. Missing, deleted
or unsupported owners leave an unresolved raw word. This classification comes from
the pinned [xEdit owner decider](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsCommon.pas#L5718-L5736).
The editor's fallback behavior does not certify the original engine's handling.

Actor `ACBS` flags and the inventory template bit, `TPLT` links, and container
`DATA` flags/weight bits are retained. A respawn bit is a source fact; it does not
schedule a respawn. The [pinned FNV schema](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas#L4183-L4226)
supplies the disk shapes and item-kind domain. The reported domain check is schema
information, not a runtime admission rule. No editor default insertion, entry
sorting, merging, template inheritance or leveled-list expansion occurs.

The [pinned runtime container layout](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameForms.h#L1023-L1048)
stores condition as a double, unlike the disk's float. This checkpoint preserves
disk bits without claiming the original conversion, live values or inventory deltas.
Selected source scopes and hashes are recorded in `sources.lock.json`; no upstream
implementation was copied or linked.

The original inspection cohort contains 9,872 records: 3,417 containers, 4,220 NPCs
and 2,235 creatures. Rust and an original offline C++ reader agree on all 235,001
fields, 30,414 item entries, 1,660 extras and 4,610 template links. The 105
nonpositive counts remain authored. All 36,686 inspected bindings are defined or
null; there are no source association findings in this cohort.

```powershell
.\target\release\fallout.exe base-inventory --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --output local/base-inventory-new.json
.\target\release\fallout-evidence.exe --checkpoint 31 --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --run-directory local/base-inventory-repeat --no-publish
```

The explicit inspection order is not the verified retail-effective load order.
Reports containing retained association findings are written and return exit code
1. Malformed field shapes, invalid extents, checksum failures and exceeded budgets
fail before publishing a report. Winning tombstones stay deleted and do not read
their body or fall back to earlier inventory entries.

Seven Rust tests cover exact words, raw-body retention, namespaces, winner
deletion, source findings and limits. The checkpoint runner independently compares
the full original cohort, authored cold/warm/reordered fixtures and fourteen
malformed cases. Fresh foreign-list, native-save, compiled-schema, quest/operand
and header/cache regressions cover shared consumers. Local reports contain source
facts; published receipts contain counts, hashes and scope without original assets.

Mutable count deltas, item instances, equipment, ownership enforcement, respawn,
template/leveled-list behavior and original `GetItemCount` coercion remain open.
No gameplay scenario is accepted by this source-format checkpoint.
