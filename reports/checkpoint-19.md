# Implementation checkpoint 19: native command operands

The script data reader now decodes vanilla native operands in instruction calls
and commands embedded in expressions. Exact numeric bits, reference indices,
variable contexts and message substitutions remain available to the next layer.
No original handler runs and no command side effect is implemented here.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 164 passing tests and Clippy with warnings denied |
| Original corpus | 14,514 compiled bodies; ten official plugins |
| Calls | 51,611 instruction calls and 16,320 expression calls; 355 command IDs |
| Operands | 80,377 regular arguments and 75 message substitutions |
| Independent comparison | Every call extent, context, typed tuple and tail hash agrees |
| Regressions | All expression envelopes/tokens and original descriptors freshly compared |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

ShowMessage retains four trailing bytes in 1,717 calls. Their meaning remains
unverified; all 6,868 bytes are preserved and hashed. Unsupported extension
encodings, bad signatures, truncations and budget exhaustion reject. A separate
original form-variable fixture covers an encoding absent from the supplied corpus.

The source snapshot covers 145 source/tooling files at implementation revision
`643b360ce66b9a42939bf9f6b77bdaea9e366651`. Its digest is
`bd32e6c010ddae9ed90094ace47fd0b613037260324ba5cf1f23cd35efb0b1d5`.
The fresh local proof is `local/native-arguments-19-verified`.

M1 remains unfinished. Native operand/table binding, conditions, actual loaded
reference values, conversions, execution effects and retail comparisons remain.
Prior rendering evidence keeps its own revision; this checkpoint does not rerun
or upgrade that acceptance. No gameplay scenario is accepted.

See [native operands](../docs/native-arguments.md),
[the scoped receipt](checkpoint-19-native-arguments.json) and
[verification](checkpoint-19-verification.json).
