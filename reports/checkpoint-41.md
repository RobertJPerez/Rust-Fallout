# Implementation checkpoint 41: source-bound script preparation

Immutable winning script handles now select bounded control/expression plans and
encoded operand associations from the same physical unit. Identical compiled
bodies with different owning tables remain separate definitions. Preparation
returns no partial plan when source or structural findings remain unresolved.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 316 Rust tests, Clippy with warnings denied; five publication tests |
| Winning definitions / compiled bodies | 80,627 / 14,476 |
| Prepared source structures | 14,420 |
| Prepared expressions / tokens / nodes | 52,038 / 145,136 / 129,626 |
| Prepared encoded operand uses | 156,273 |
| Retained source findings | 53 control, three expression, three metadata findings |
| Absent compiled fields without metadata findings | 66,148 |
| Independent comparisons | Every winning handle/version/owner, compiled body, prepared structure and own-table binding |
| Fresh regressions | Full control/expression/form-list/query/source/runtime/cold-save checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The proof binds 289 source/tooling files at `67681fade406e43266f306ab460d96324acd27dd`,
digest `d8c468f4a41629c59872e59928b8e7f88ce8c235a0b046affa22226135bdbc8a`. Raw evidence remains local:
`local\source-plans-41-verified`.

Preparation checks encoded source associations; deferred foreign owners and
dynamic reference values still require explicit live bindings. It does not
initialize locals, admit native form domains, decode event arguments or grant
execution permission. Event lookup uses the exact begin-header byte offset;
an event ID alone cannot choose a block. Missing bodies remain explicit.

Original arithmetic, branch rules, native effects, scheduling, event lifecycle
and observable execution remain open. M1 and gameplay acceptance are unfinished.
Next: bounded runtime components over the immutable source catalogue and plans.

See [source-bound plans](../docs/source-bound-script-plans.md),
[scoped receipt](checkpoint-41-source-plans.json),
[verification](checkpoint-41-verification.json) and [remaining gates](../NEXT_STEPS.md).
