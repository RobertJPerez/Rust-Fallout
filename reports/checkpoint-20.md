# Implementation checkpoint 20: compiled operand table associations

The script layer now associates supported encoded indices with their owning SCHR
unit's local and reference tables. Foreign local indices retain their reference
context and defer declaration lookup to the loaded target script.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 167 passing tests and Clippy with warnings denied |
| Corpus | 14,514 compiled units across ten official plugins |
| Associations | 159,111 exact typed/source associations agree independently |
| Current locals / SCRO forms / SCRV locals | 26,888 / 102,895 / 3,768 |
| Foreign declarations deferred | 25,560; no guessed current-script substitution |
| Missing entries / compiled decode issues | 0 / 0 |
| Regressions | 80,736 table units and all prior native/expression/descriptor scopes freshly compared |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

Three original empty-unit reference counts remain inconsistent. They produce a
diagnostic report and unsuccessful CLI status; none has compiled-byte issues.
Conflicting repeated declarations keep their prior retail-loading gap.

The source snapshot covers 155 source/tooling files at implementation revision
`109cee6a2dd8bfba8f79884983801e214d8c1c56`. Its digest is
`3e434302ff284487af442b24c50ce08fd3bc8d3f3a1b35fea1f0a155dad77a2d`.
The fresh local proof is `local/operand-bindings-20-verified`.

M1 remains unfinished. Loaded forms/scripts, conditions, expression evaluation,
event/control-flow behavior and native effects remain open. This checkpoint does
not rerun or upgrade prior rendering acceptance. No gameplay scenario is accepted.

See [operand associations](../docs/operand-bindings.md),
[the scoped receipt](checkpoint-20-operand-bindings.json) and
[verification](checkpoint-20-verification.json).
