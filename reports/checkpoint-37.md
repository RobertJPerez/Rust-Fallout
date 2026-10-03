# Implementation checkpoint 37: preserving verified metadata

The initial census generator now stages historical outputs in a new local
directory. It cannot reset current profiles, registries or verified ledger rows.
Directory and file publication use exclusive creation.

| Evidence | Result |
| --- | --- |
| Rust workspace | Formatting, 283 passing tests and Clippy with warnings denied |
| Publication tests | Five pass through the standard verification workflow |
| Actual bootstrap fixture | Seven generated JSON files; current metadata and input bytes unchanged |
| Occupied targets | Existing directories/files rejected without replacement |
| Target scope | Repository/external paths rejected; new direct local child required |
| Fresh query regression | Twelve shared entry traces agree after cold restore |
| Source regressions | All 640 descriptors and 80,467 winning CTDA bindings independently compared |
| Runtime regressions | Complete source-item/save migration/item/leveled/inventory/foreign/schema/quest/operand/header checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The source snapshot covers 263 source/tooling files at revision
`ee0c67bb3b019bc316c22abd75e32b996508a586`, digest `263bebddd9a92d7585693e9f0c5cd8810e2bc4188074e54bf5f46dac2bfac2b1`. Fresh proof:
`local/report-publication-37-verified`.

The bootstrap test values are authored fixtures. This checkpoint protects evidence;
it does not accept original gameplay. M1 remains unfinished. Original count/list
coercion, VM/native execution, actors/AI/combat, complete campaign state and retail
comparisons are still open.

See [publication workflow](../docs/report-publication.md),
[scoped receipt](checkpoint-37-report-publication.json),
[verification](checkpoint-37-verification.json) and [next steps](../NEXT_STEPS.md).
