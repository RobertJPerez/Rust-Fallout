# Implementation checkpoint 31: base inventory source declarations

The immutable catalogue now reads winning container, NPC and creature inventory
entries. Signed counts, duplicate items, ownership words and template inputs remain
exact source facts, ready for later live inventory components.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 238 passing tests and Clippy with warnings denied |
| Original definitions | 9,872: 3,417 CONT / 4,220 NPC_ / 2,235 CREA |
| Independent source comparison | All 235,001 fields and 30,414 item entries agree |
| Source extras/templates | 1,660 COED fields / 4,610 TPLT links |
| Nonpositive counts | All 105 retained without normalization |
| Binding facts | 35,026 defined / 1,660 null; exact independent agreement |
| Fixtures | Cold/warm/reordered inputs agree; three ambiguity findings retained |
| Malformed sources | Fourteen cases rejected by both readers without reports |
| Fresh regressions | Foreign live lists, native saves/schemas, quest/operand and loaded/header/cache checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The source snapshot covers 239 source/tooling files at implementation
revision `81879010ef23ab5d0e5e0e0754dac5df845ef804`. Its digest is
`a10f3c42f0fdc54677cce42204f5f5f768416c43e2541f781eba77a067fe6537`. The fresh proof is
`local/base-inventory-31-verified`.

M1 remains unfinished. No live items were initialized, and no gameplay scenario is
accepted. Next: leveled-list source inputs and bounded dependency planning, then
mutable item state and original primitive-query behavior.

See [base inventory](../docs/base-inventory.md),
[the scoped receipt](checkpoint-31-base-inventory.json) and
[verification](checkpoint-31-verification.json).
