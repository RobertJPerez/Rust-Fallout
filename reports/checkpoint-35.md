# Implementation checkpoint 35: source-bound item validation

Host item mutations now check supplied content links against winning headers and
explicit caller kind rules. Missing/deleted forms and disallowed kinds fail before
canonical mutation; absent rules do not silently allow every kind.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 275 passing tests and Clippy with warnings denied |
| Runtime tests | Eight cover all roles, policy bounds/order, atomic failures, overrides, tombstones and namespaces |
| Source index | All 628,464 winning classifications match independent original headers |
| Validated occurrences | 10 final form occurrences match independent source bindings |
| Engineering mutations | Add nine units and replace their base; four final item instances |
| Query results | 3 / 0 / 21 / 3 / 0 / 0 |
| Rejections | Missing key, actual deleted winner and deliberately disallowed kind preserve state |
| Cold restore | Full policy, source facts, item proofs, query traces and canonical digest agree |
| Container | Independent metadata/extents/checksums agree |
| Fresh regressions | Complete native migration/item/leveled/inventory/foreign/schema/quest/operand/header checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The canonical snapshot is 2,383,543 bytes with SHA-256
`3437bf23d645288089e45ed487fe63bc4fd583db2fcbc487d4ef2ca085e580f4`. Caller policies and mutable values are disclosed
engineering inputs; they do not establish original runtime admission.

The source snapshot covers 258 source/tooling files at revision
`65d9c933fa176e5606df9b640d6c88cd47e07905`, digest `95072ad3ed501c3c655b897fbf761eda3adeeb075d187c19dc9fc937836a86c0`. Fresh proof:
`local/source-items-35-verified`.

M1 remains unfinished and no gameplay scenario is accepted. Next: shared primitive
query interfaces and measured original return/execution behavior.

See [source checks](../docs/source-item-validation.md),
[scoped receipt](checkpoint-35-source-items.json) and
[verification](checkpoint-35-verification.json).
