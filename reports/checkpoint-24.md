# Implementation checkpoint 24: immutable winning script definitions

The content store now loads immutable script definitions with canonical unit
identities, source-version handles and explicit reference dependencies. Authored
locals stay sparse and ordered; embedded scripts retain their own source owners.
Deleted winners never resurrect earlier definitions, and stale handles reject.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 186 passing tests and Clippy with warnings denied |
| Winning scripts / compiled bodies | 80,627 / 14,476; every loaded row and version digest agrees |
| Declarations / references | 11,183 / 51,034; sparse/duplicate indices and all reference states preserved |
| Retained decoded records | 39,981 / 22,469,823 bytes shared across script units |
| Completeness | Full bound scan has 80,736 units; native winner filtering excludes 109 and matches all 80,627 |
| Original source findings | Same three stale SCHR counts; no new ownership findings |
| Independent fixtures | Five scripts with all seven reference states, duplicate/zero variables and distinct owners |
| Malformed catalogue inputs | Twelve native cases rejected without a report |
| Shared header reader regression | Complete checkpoint 23 header/membership/cache/cell/source-fixture comparison freshly passed |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

The independent native reader reads original headers and uncompressed payloads
directly under write-denying source handles. Compressed payload extraction remains
Rust-owned; the native reader checks original decoded-size prefixes and verifies
completeness against the bound full source-table scan. Its winning metadata digest
is `e454dfd982e0fee60a87edca4526be655a0a217dc9b76b000273ef007993dd35`.

The source snapshot covers 184 source/tooling files at implementation
revision `cfd1276eb382ca457df27e105a3c7f0f9093ab8e`. Its digest is
`8a668fdce3d4693e20bedead80124afe23c7f0f8fcdebb1bc0b4c43e831b78cf`. The fresh local proof is
`local/loaded-scripts-24-verified`.

M1 remains unfinished. Quest attachments and foreign declaration lookup are next.
Live event lists, dynamic values, scheduling, command effects, condition queries,
dialogue selection and retail acceptance remain open. The inspection order remains
separate from the original game's unmeasured effective order. No gameplay scenario
is accepted.

See [loaded scripts](../docs/loaded-scripts.md),
[the scoped receipt](checkpoint-24-loaded-scripts.json) and
[verification](checkpoint-24-verification.json).
