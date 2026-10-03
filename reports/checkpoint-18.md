# Implementation checkpoint 18: compiled expression envelopes and tokens

The Rust tokenizer preserves original decimal lexemes, quoted bytes, context
prefixes, operator/command IDs and opaque native argument payloads. It requires
no script source text and does not evaluate or execute an expression.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 158 passing tests and Clippy with warnings denied |
| Source | 136 source/tool/configuration files match implementation commit e80e9f2 |
| Corpus | All 14,514 authored compiled bodies covered |
| Independent envelopes | All 53,404 set/if/elseif envelopes agree |
| Independent tokens | All 149,082 token tuple digests / 538,070 expression bytes agree |
| Embedded commands | 16,320 calls across 143 IDs bind to vanilla descriptors |
| Operator metadata | All 16 descriptors independently read from the fingerprinted executable |
| Prior descriptor layer | All 694 descriptors and 638 parameter occurrences freshly compared |
| Edge cases | 16 independent negative bundles reject; quoted-byte fixture agrees |
| Original data | Strict LAND failure retained; all 464 installed files match baseline |

The supplied expressions have zero trailing operand bytes and zero pending
context prefixes. Both fields remain retained for other inputs. No quoted
expression token occurs in the corpus; its support has original synthetic
coverage. Unknown n/z expression tokens reject, even though native argument
formats use those bytes with other meanings.

Independent token parsing starts at Rust-extracted SCDA. Decimal boundary
comparison uses the current CRT and does not certify original values, rounding,
locale behavior or evaluation. Native arguments, expression table binding,
postfix semantics, jumps, scheduling and native effects remain unfinished.
No retail or campaign/gameplay scenario is accepted.

See [compiled expression inspection](../docs/script-expressions.md),
[the scoped receipt](checkpoint-18-script-expressions.json) and
[verification](checkpoint-18-verification.json). Earlier table binding, terrain,
GPU, collision and cache evidence keeps its own tested revision. The descriptor
layer was freshly compared; presentation checks were not reexecuted here.
