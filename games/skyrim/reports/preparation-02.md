# Skyrim preparation checkpoint 02

Verified October 3, 2026 (America/New_York), against installed Steam runtime
**1.7.104.0**, build **24914197**. This advances the content preparation in
[checkpoint 01](preparation-01.md); there are still zero accepted gameplay scenarios.

Skyrim now has bounded VMAD fragment and quest-alias decoding, source-addressed
binding exports, independent field/payload comparisons, and explicit diagnostics
for source references that cannot yet be resolved. The existing pinned NV code
still owns record framing, decompression, path handling, source locking and
hashing. The Fallout 4 project still owns PEX decoding; no second PEX reader,
renderer, simulation or save implementation was introduced.

| Measured scope | Result |
| --- | ---: |
| Installed plugins / BSA archives | 10 / 23 |
| Physical records including plugin headers | 1,188,920 |
| VMAD declarations decoded | 24,631 |
| Record-specific tails decoded | 10,178 |
| Prefix / tail decoding failures | 0 / 0 |
| Fragment bindings, preserving source order | 18,230 |
| Quest alias entries | 3,228 |
| Primary / alias properties preserved | 73,386 / 7,060 |
| Distinct required script paths | 13,279 |
| Independently matched complete VMAD declarations | 24,630 / 24,631 |
| Independent field and structure comparisons | 1,242,554 |
| Independently matched archived PEX members | 14,302 / 14,302 |
| Independently extracted script bytes | 17,650,570 |
| Rust tests passing | 27 |

The tails cover INFO events (8,120 fragments), PACK events (605), PERK entries
(22), QUST stage/log entries (7,940), SCEN events (739) and SCEN phases (804).
VMAD v4/v5 and both object layouts occur in the corpus. Unsupported versions,
flag bits, mixed alias layouts, truncation, excessive counts and trailing bytes
stay explicit failures. Unknown tails retain their original bytes.

Two stale filename hints initially appeared to imply missing scripts. Their
records contained no executable fragments. The dependency catalog now records
filename hints separately and requires only actual fragment bindings and
nonremoved primary/alias attachments. A regression test covers that distinction.
The seven absent executable script paths from checkpoint 01 remain unchanged.

The strict independent VMAD comparison has **one unresolved difference**:
Dawnguard ACTI `02014760`, `DLC1VampireLordBlockerSCRIPT.Collision`, stores
`0307B5B9`. It exceeds the plugin's two-master source index range. Mutagen's
fallback maps it to `0207B5B9`; the Rust reader preserves the original bits.
A direct read at physical byte 786615 confirmed `B9 B5 07 03` in that uncompressed
record. The census reports this finding, and the strict oracle intentionally
returns 2. No retail file was repaired and no runtime fallback was assumed.
The final census therefore returns 2 for eight input findings, despite zero
decoding/open failures. See the [binding contract](../docs/binding-contract.md).

Independent archive verification passed for all 23 BSAs: 172,961 entry counts,
extension histograms, duplicate-path counts, and every script's path, extracted
length and SHA-256 agree. Non-script payloads were not independently decompressed
or interpreted. The archive input reports are identical between the compared
census and the final census. The esplugin metadata comparison also passes all
10 plugins. Original plugin/archive/executable hashes remain unchanged.

Validation includes Rust formatting and Clippy with warnings denied; .NET
validation tools build with warnings as errors and restore locked packages.
Deliberately changing one fragment function-name byte produces exactly one
comparison failure with all counts unchanged. Changing one expected script hash
also fails. Both mutation controls first require their unaltered input to pass.

Evidence remains under ignored local paths:

- `local/census-20261004T015436Z-fb0e2b91.json` and matching installation receipt.
- `local/bindings-20261004T015645Z-96afe0c9/receipt.json`, per-plugin JSONL exports
  and independent comparisons.
- `local/plugin-oracle-02.json` and `local/archive-oracle-02-probe.json`.
- `local/negative-vmad-20261004T015217Z-49cc837e/receipt.json`.
- `local/negative-archive-20261004T015411Z-ff844ae6/receipt.json`.

Exact source and evidence hashes are in [preparation-02.json](preparation-02.json)
and [source-manifest-02.json](source-manifest-02.json). The paid Anniversary
content collection is unavailable and untested; its ownership is not inferred
from the four installed Creation-named plugins. Current SE/AE data preparation
does not certify Papyrus execution, quests, SKSE DLLs, retail saves, rendering,
physics or the original Helgen/new-game route. Those require the shared runtime
and separate behavioral evidence described in [next steps](../NEXT_STEPS.md).
