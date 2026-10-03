# New Vegas condition fields

Rust now decodes original CTDA fields without evaluating a condition. Comparison
operators, float/global comparison words, function IDs, raw parameter words,
optional subject/reference words, padding and source order remain available to
the quest/dialogue loader.

```powershell
target/release/fallout.exe conditions --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --output local/condition-fields-new.json
```

The focused scan covers FNV schema records containing conditions, directly or
through effect lists. Its complete field/function/length counts are checked
against the earlier full corpus inventory. Unrelated bodies remain deferred;
without that option, the strict scan still rejects the installed LAND checksum
defect. Comparison bundles stay ignored locally and outside the installation.

Pinned [xEdit FNV definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
provide the on-disk layout, optional fields and the special run-on domains for
IsFacingUp and IsLeftUp. The inspected comparison helpers in
[common definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsCommon.pas)
describe operator bits and float/global discrimination. These selected routines
are format evidence; no upstream implementation is copied or linked.

CTDA has verified lengths of 20, 24 and 28 bytes. Missing run-on/reference words
remain absent. The function field is u16 plus two preserved bytes on disk;
xNVSE's u32 in-memory field is a different boundary. Float values retain exact
bits, and global comparison words retain raw source FormIDs. Parameter meanings
are function-specific and are not inferred from native argument classifications.

The OR bit remains authored metadata. Consecutive CTDA fields do not automatically
form one executable condition list: different quest stages, effects, perk entries
or message buttons can own separate lists. Preceding field kinds and decoded
offsets are recorded, but ownership and grouping need their typed record schemas.
No query is treated as true or false because its behavior is missing.

All ten official plugins contain:

| Check | Count |
| --- | --- |
| Condition fields / containing records | 80,627 / 32,699 |
| Distinct original condition function IDs | 148 |
| Twenty / twenty-four / twenty-eight byte layouts | 123 / 2 / 80,502 |
| Authored OR flags / global comparisons | 3,730 / 59 |
| Present active run-on reference words | 2,990 |
| Unknown comparison codes / nonfinite float comparison words | 0 / 0 |

All 148 function IDs bind to independently verified original vanilla descriptors
with condition handlers. This names original metadata; our condition handlers
remain unimplemented.

Two original records retain unexplained fields. FalloutNV INFO `0013AC86` has
low flag `0x10` and nonzero flag padding in its condition at decoded offset 297.
FalloutNV IDLE `00019E2F` has run-on word `7` at decoded offset 97, outside the
inspected subject-selection enum. Their exact fields and hashes remain in the
receipt. Legacy short-layout loading/migration and these fields' retail handling
remain unverified. No source file or original field is repaired.

The original offline `tools/condition-oracle` independently reads all CTDA fields
from Rust-extracted decoded records. Every row, optional word, raw union value,
preceding-field/source extent and aggregate count agrees. This starts after Rust
plugin extraction/decompression, and does not establish grouping, migration,
numerical comparison or original query behavior. The executable catalogue is
freshly compared too; no original executable code runs.

Original fixtures exercise all flag bytes, short layouts, nonfinite/unknown
patterns, preserved padding, exact field order, malformed lengths and unrelated
integrity failures. Next work is function-specific parameters, typed quest/dialogue
ownership, actual subject/reference resolution and independently verified queries.
No condition evaluation or retail gameplay scenario is accepted.
