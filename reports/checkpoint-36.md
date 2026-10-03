# Implementation checkpoint 36: shared primitive query routing

The verified GetItemCount native and condition entry IDs now use the same exact
host inventory count trace. Subjects and content arguments are explicit. Original
numeric return conversion is absent, and form-list expansion fails explicitly.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 283 passing tests and Clippy with warnings denied |
| Runtime tests | Eight cover typed requests, wide counts, bounds, current state and restore contexts |
| Original entry | Command 4143 / function 47; one required type-50 ObjectID and parent |
| Shared calls | Twelve agree on all canonical core trace fields |
| Cold restore | Every complete entry trace agrees |
| Canonical state | Read-only queries leave it unchanged |
| Command registry | All 640 independently verified executable descriptors refreshed |
| Condition registry | 148 observed functions / all 80,467 winning CTDA fields independently compared |
| Fresh regressions | Complete source-item/native migration/item/leveled/inventory/foreign/schema/quest/operand/header checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

Exact host query results are 3 / 0 / 21 / 3 / 0 / 0. These are engineering inputs,
not original live counts. No handler, VM bytecode, numeric conversion or condition
comparison executes. Both registries keep original behavior status unknown.

The canonical snapshot has 2,383,543 bytes, SHA-256
`3437bf23d645288089e45ed487fe63bc4fd583db2fcbc487d4ef2ca085e580f4`. Source/tooling snapshot: 262 files at
`e2bf84d7f4d7289822f68efbcb8fdcac0f330b5d`, digest `4594bd585f4afe16d2debf6ec82bfaebdec5c7772d20881e94326bd22ea2baf4`. Fresh evidence:
`local/primitive-queries-36-verified`.

M1 remains unfinished and no gameplay scenario is accepted. Next: measured
original count/list/coercion behavior and bounded observable VM execution.

See [primitive queries](../docs/primitive-queries.md),
[the scoped receipt](checkpoint-36-primitive-queries.json),
[verification](checkpoint-36-verification.json),
[commands](../parity/commands.json) and [conditions](../parity/conditions.json).
