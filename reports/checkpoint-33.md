# Implementation checkpoint 33: canonical item state

Explicit item banks now preserve persistent identities, quantities and optional
attributes. Atomic mutations and indexed count traces round-trip through native
saves. Unknown original facts remain uninitialized rather than receiving defaults.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 261 passing tests and Clippy with warnings denied |
| Item operations | Add, split, transfer, quantity removal and fact replacement |
| Runtime tests | Fourteen; includes 2,000 operations checked against canonical sums |
| Original identities | Three base-inventory bindings agree with independent original-source fields |
| Explicit final state | Three items in two initialized banks; 2,483 script instances |
| Query results | 3 / 0 / 12 / 3 / 0 / 0; every cold trace agrees |
| Capture | Owned worker state excludes later host mutation |
| Persistence | Schema 3; exact condition widths/bits, optional facts and opaque fields |
| Independent container | Metadata, extents and SHA-256 agree with C++ |
| Fresh regressions | Complete leveled/base-inventory/foreign/native-save/schema/quest/operand/header checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The canonical snapshot has 2,383,275 bytes and SHA-256
`6bce609cbda63f052961c6ba9e7d42740efcfe7965bb079a604bdf6b5a325d3f`. Values are disclosed engineering inputs. Source and
container comparisons do not accept original initialization or mutable behavior.

The source snapshot covers 251 source/tooling files at revision
`d401473c6e43d4bc9b03edc0554998c4254ff1cc` with digest `abc0f98cc2fe3343b39fe346da9f8ded5bb394549a4c8e38e86e2f030b3d64d4`. The fresh proof is
`local/item-state-33-verified`.

M1 remains unfinished and no gameplay scenario is accepted. Next: explicit old
native-envelope import, source admission and shared primitive query adapters.

See [item state](../docs/item-runtime-state.md),
[the scoped receipt](checkpoint-33-item-state.json) and
[verification](checkpoint-33-verification.json).
