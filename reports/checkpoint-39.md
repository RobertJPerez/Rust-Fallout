# Implementation checkpoint 39: bounded expression structure

Vanilla source tokens now produce flat postfix plans with ordered children,
semantic subtree ranges and explicit node/stack/height limits. Independent Rust
forward and C++ backward construction agree on every source plan and finding.
Strict mode rejects incomplete structure; diagnostic mode retains it and returns 1.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 300 Rust tests, Clippy with warnings denied; five publication tests |
| Original compiled bodies | 14,514, bound to the fresh extraction/source cohort |
| Original expression statements | 53,404; every source/token/expression identity compared |
| Complete structural plans | 53,401 / 133,235 nodes |
| Maximum original stack / height | 6 / 14 |
| Retained source findings | Three INFO expressions leave two operands under the selected model |
| Independent comparison | Exact ordered shape tuples, subtree ranges and structured findings |
| Positive fixtures | Six, including a nonrecursive 50,001-node chain |
| Structural negatives | Nine strict rejections and equal diagnostic comparisons |
| Debug CLI startup | Freshly rebuilt executable runs --help successfully |
| Fresh regressions | Form lists, shared queries/CTDA/descriptors, source items/native saves/item state/leveled/inventory/foreign/schema/quest/operands/headers |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The proof binds 278 source/tooling files at `222479b882f9af11b16a042d5909f31ec8f51afb`,
digest `ff2191ca643b946100192b94e2d17a15351036c0693c4a6debe6b98468ed38a6`. Raw local evidence:
`local\expression-plans-39-retry1-verified`.

The source findings belong to FalloutNV.esm INFO forms 001588E5, 001588E6 and
00160731. They are retained with source/record/SCDA identities and exact hashes.
Original handling is unmeasured; no source repair or runtime exception is adopted.

Operator structure does not establish numeric effects, short-circuit execution,
native returns, branch origins, event scheduling or script effects. M1, VM and
gameplay acceptance remain unfinished. The original compiled bytes stay local.

See [expression plans](../docs/expression-plans.md),
[scoped receipt](checkpoint-39-expression-plans.json),
[verification](checkpoint-39-verification.json) and [remaining gates](../NEXT_STEPS.md).
