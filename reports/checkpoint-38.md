# Implementation checkpoint 38: source form lists

The immutable catalogue now reads winning FLST physical fields and ordered LNAM
members. Whole-record overrides, tombstones, duplicate occurrences and unresolved
bindings remain explicit. A bounded structural graph shares the iterative Rust
traversal; a separate original-source C++ reader uses a different SCC algorithm.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 291 Rust tests, Clippy with warnings denied; five publication tests |
| Original lists | 608 winning lists, 6,698 fields, 78,042 decoded bytes |
| Original members | 6,090 occurrences; every binding agrees |
| Original graph | 608 nodes, 6,090 edges; two nested links, 6,088 terminals; no cycles/unresolved edges |
| Independent comparison | Every physical field/hash, member, source, target and graph/SCC matches |
| Authored fixtures | Cold/warm equality and expected reuse; changed patch invalidates only its index |
| Malformed inputs | Ten cases reject in both readers without a successful report |
| Graph/API limits | Eight source tests plus shared 100,000-node chain/cycle traversal tests |
| Fresh regression | Complete shared query, winning CTDA/descriptors, source-item/native/item/leveled/inventory/foreign/schema/quest/operand/header chain |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The proof binds 271 source/tooling files at `05d782c74bf9c363c49ae4ea9b8a0b2d2dd4db50`,
digest `c1072c4aedafdbff1e2c1a7d10c9fbaab58cb4cdcf23d1b20bdfab1e69e0c456`. Local raw evidence:
`local\form-lists-38-retry1-verified`.

GetItemCount still rejects unverified form-list expansion. Authored members do not
establish live script-added state, list flattening/duplicates/null behavior, original
argument coercion or retail numeric return conversion. M1 and gameplay acceptance
remain unfinished; the other games retain their later dependency gates.

See [form lists](../docs/form-lists.md), [scoped receipt](checkpoint-38-form-lists.json),
[verification](checkpoint-38-verification.json) and [remaining gates](../NEXT_STEPS.md).
