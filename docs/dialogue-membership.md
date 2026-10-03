# Winning dialogue topic membership

INFO now retains its original topic-child GRUP label in the record index. The
membership layer resolves that label through the winning INFO source's master
table, then checks the winning target is a live DIAL record. Moved overrides use
their new parent; deleted winners never fall back to an older definition.

```powershell
target/release/fallout.exe dialogue-membership --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --output local/dialogue-membership-new.json
```

The supplied order is explicit inspection input. The original game's effective
order remains unmeasured. Header-only indexing validates TES4/CELL metadata and
defers other bodies to strict access. A linked INFO header does not certify its
dialogue text, conditions or scripts.

`MembershipIndex` provides bounded lookup of canonical INFO keys for a topic.
Its lists are sorted by canonical key for reproducibility, not retail response
order. Missing/null parents, missing/deleted/wrong-kind topics and deleted INFO
tombstones remain distinct statuses. Unresolved live links produce a diagnostic
report and unsuccessful CLI status.

Under the supplied ten-plugin order:

| Check | Count |
| --- | --- |
| Source record definitions / winning definitions | 629,788 / 628,464 |
| Winning INFO records with live DIAL parents | 28,896 |
| Topics containing linked INFO records | 11,994 |
| Missing/null/deleted/wrong-kind live topic links | 0 |

The original offline `tools/record-oracle` reads plugin headers directly from the
installation under read locks. It independently frames groups and record extents,
reads TES4 master tables, converts source FormIDs and rebuilds whole-record winners.
It compares every indexed header/canonical-key/parent-context digest, every winning
INFO row and topic membership list, and all source hashes. It does not use
Rust-extracted record bundles or execute original code. Record payloads other than
TES4 remain deferred in this native comparison.

The metadata digest encodes origin-name length/bytes (u16), local ID (u32), source
name length/bytes (u16), signature, file offset (u64), stored length/flags/raw ID
(u32 each), revision bytes, version (u16), two trailing bytes, then four tagged raw
parent values: topic, world, cell and child group. Each parent uses a presence byte
and a u32 value; absent values encode zero. The profile is explicitly NV original.
Source definitions use supplied-plugin/file order; winners use canonical-key order.
Editor IDs and body fields are outside this digest.

The [pinned xEdit group implementation](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbImplementation.pas)
identifies group type 7 as topic children and treats its label as a source FormID.
The existing independently checked record framing and namespace rules remain in
use. No upstream implementation is copied into the runtime or original oracle.

Index caching now uses FNVHIDX version 2 and a new transform identity. It stores the
additional topic parent value. Version 1 bytes cannot be decoded as version 2;
existing version 1 artifacts remain separate. Uncached, cold and warm membership
reports agree exactly, and a damaged separate cache copy is rejected before a
complete report is written. The Doc Mitchell selected-cell cache comparison is
also rerun after this format change.

Original fixtures independently exercise absent/null/missing/wrong-kind/deleted
links, unrelated plugin reordering, master-relative parent rebasing, moved INFO
overrides, deleted topics and world/cell parent contexts. Malformed order bundles
and source headers fail without emitting complete reports. Existing cache tests
retain process-termination recovery and strict deferred-payload checks.

Record-specific override exceptions, original dialogue ordering/selection,
condition queries, script timing and actual gameplay acceptance remain unfinished.
