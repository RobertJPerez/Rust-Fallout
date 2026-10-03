# Static quest script attachments

The `quest-scripts` inspector follows each winning QUST record's authored SCRI
field into the immutable script catalogue. It preserves missing, null, deleted,
wrong-kind, unowned and ambiguous states. A deleted winning script never falls
back to an older definition. A quest with multiple SCRI fields does not select
an arbitrary one. Lookup is bounded and uses canonical record-key ranges rather
than scanning the whole catalogue for each quest.

```powershell
.\target\release\fallout.exe quest-scripts `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --script-comparison-bundle local/quest-scripts.bin `
  --quest-comparison-bundle local/quest-attachments.bin `
  --output local/quest-scripts.json
```

The CLI publishes source findings before returning 1. The original inspection
corpus retains three stale empty-unit reference counts and two short QUST headers.
No header repair, migration, guessed script attachment or runtime default is used.

Foreign operands first select their context from the owning script's one-based
reference table. For a defined quest, the inspector follows that quest's static
SCRI attachment and looks up the foreign local index in the attached script.
The first authored declaration wins; duplicate and sparse declarations remain
intact. An equally numbered local in the current script is never substituted.
All type bytes and names remain source metadata; reports publish name hashes.

The pinned xNVSE source distinguishes two relationships. Its
[GetReferencedScript](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameScript.cpp)
uses the quest's scriptable script for a quest and ExtraScript for a placed
reference. Its
[GetParentScript and ResolveExternalVar](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameAPI.cpp)
select a live event list when accessing variables. Static declarations can now
be associated under the authored quest relation; which script and value a live
event list currently supplies is a separate runtime question.

Placed references therefore retain `placed_reference_needs_event_list`. The
inspector does not substitute a base-object script or an embedded REFR unit.
Dynamic SCRV contexts, hardcoded player references, missing/deleted/null forms,
unavailable quest scripts and missing foreign declarations remain explicit.
Every lookup has `live_value_resolved: false`. Source/target handles are checked
for staleness. Mixing attachment data with another winning quest source produces
`quest_winner_mismatch` rather than an association across different winners.

The original offline C++ tool reads original plugin headers and source hashes,
rebuilds namespaces and whole-record winners, checks complete winning QUST body
coverage, and independently reconstructs attachments and foreign declarations.
It compares every loaded operand binding against its own operand reader. Original
executable metadata is read under a write-denying handle and fingerprinted; no
original executable code or native handler is loaded or invoked.

The shared original C++ operand helper receives a fresh complete source-scan
regression, including table, native argument, expression and executable-descriptor
comparisons. The shared Rust input/bundle helpers receive a fresh complete loaded
catalogue/header/membership/cache comparison. Every winning compiled unit also
matches its metadata, SCDA and operand digest from the full source operand scan.

`FRQUEST1` is a local-only winning quest-body bundle. After its eight-byte magic,
each record uses source index u8, kind four bytes, raw FormID u32, flags u32,
original file offset u64, decoded payload length u32 and payload. Source indices
name the explicit inspection order. The format is bounded at 256 MiB. Raw bundles,
source text and the original executable stay in ignored local evidence.

The inspection order remains unmeasured as a retail effective order. Compressed
extraction remains Rust-owned. Script scheduling, variable values, command effects,
quest progression, condition evaluation and retail gameplay acceptance remain open.
