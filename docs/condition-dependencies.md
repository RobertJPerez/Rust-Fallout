# Typed condition operands and dependencies

The `condition-dependencies` inspector loads CTDA fields from whole-record winners
and resolves their static FormID dependencies. It keeps each field attached to its
record and decoded offset. It does not combine conditions from different quest
stages, dialogue sections, effects or perk entries.

```powershell
.\target\release\fallout.exe condition-dependencies `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --output local/condition-dependencies.json
```

The exact fingerprinted executable supplies condition-handler presence and native
parameter IDs. The inspected
[xEdit FNV definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
supply CTDA word types, optional layouts and the VATS selector union. Native
argument prefixes and widths are not applied to these four-byte condition words.
The inspected
[xNVSE parameter definitions](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/CommandTable.h)
name the descriptor IDs; its in-memory condition structure is a separate boundary.

Signed integers, signed domains, unsigned domains, variable indices and exact float
bits remain distinct. The VoiceType condition has an explicit inspected association.
`GetVATSValue` changes its second word's type according to the first word: some
selectors use FormIDs, others use enumerations, and several leave the word unused.
Unused nonzero bytes remain visible. Unknown selectors, missing descriptors,
unverified optional words and signatures with extra parameters remain unresolved.

Source namespaces select canonical keys; winning headers supply target provenance.
Null, defined, deleted, missing and runtime-dependent forms remain separate. A
deleted target never selects an earlier definition. The hardcoded player reference
is a runtime dependency only when no authored winner exists. Defined dependencies
do not certify that a native handler accepts the target's record kind. Reports
explicitly retain `target_kind_acceptance_checked: false`.

Subject, target, explicit reference, combat target and linked reference are distinct
runtime requests. The two animation-group functions retain their separate run-on
domain. Absent 20/24-byte-layout words stay absent. The old target flag stays an
authored flag; xEdit's after-load expansion and target migration are not applied.
Global comparison values resolve only their static FormID. All variable indices,
global values and selected subjects still need live state. Query evaluation,
AND/OR grouping and retail defaults remain unfinished.

The original offline C++ reader independently walks whole-record winners and
reads their original payloads directly. Compressed payloads use checkpoint 26's
separate RFC decoder with strict checksums. It independently reads the executable
descriptors and reconstructs every condition word, static dependency, source
finding and per-record binding digest. All source files remain under write-denying
handles. No original executable code or handler is invoked.

Fresh proof compares every original decoded record digest and binding row. Winning
coverage is also filtered from the immutable, hashed full checkpoint 21 field scan;
the bound complete source census supplies master tables. A fresh descriptor
comparison checks the executable catalogue. Authored plugin fixtures exercise VATS
branches, short layouts, all dependency states, unknown descriptors, nonwinning
records, tombstones, cache build/reuse and unrelated-plugin reordering. Malformed
fields, extended sizes and selected compressed checksums abort without a report.

The report returns 1 after publishing source or schema findings. The original
corpus retains the same two records with unverified flags/padding or a subject
selector. Runtime values are never fabricated, and no gameplay scenario is accepted.
