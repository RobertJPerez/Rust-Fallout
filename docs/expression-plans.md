# Bounded vanilla expression structure

The source token decoder now feeds a flat postfix plan in
`fallout-data::obscript::expression_plan`. The plan retains borrowed raw bytes and
read-only tokens, ordered children, contiguous semantic subtree ranges, height and
maximum operand-stack size. Local/global/reference operands and opaque commands
remain distinct source tokens. A reference prefix supplies metadata to its later
local/command consumer; it does not become a value on the operand stack.

The structural model checks all sixteen operator IDs/spellings against the
caller-supplied descriptor table. Unary tilde and binary operand order follow the
selected pinned analyzer routines. Parenthesis operators remain unsupported as
compiled postfix instructions. Precedence is preserved in tokens, without sorting
an already authored postfix sequence. Extension tables require another verified
model. No source or runtime value is coerced or evaluated.

Construction, storage, traversal ranges, hashing and destruction use an iterative
arena. Limits cover input bytes, tokens, literals/lexemes, nodes, stack and height.
Empty/residual stacks, operand underflow, dangling/replaced prefixes and unknown
models fail explicitly. A successful plan owns its private token stream; public
display counters or forged tokens cannot replace it. Each fixed 33-byte shape
tuple binds the source token index/extents, arity, operator ID, ordered child
indices, subtree start and height. Token/expression hashes bind the payload bytes
which are deliberately absent from shape tuples.

The existing `FROBS001` interchange file carries raw SCDA bodies only. Its complete
hash and each body's size/hash must match a fresh plugin extraction receipt before
the checkpoint assigns source provenance. The inspector does not infer a plugin
identity, winning script handle or runtime context from that file. Bundles, body
sizes/counts, aggregate statements/nodes and instruction counts are bounded. Malformed framing, token layouts
or uninterpreted statement tails fail without a successful report.

```powershell
.\target\release\fallout.exe expressions `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --defer-unrelated-payloads --comparison-bundle local/plans-new.bin `
  --output local/plans-extraction-new.json

.\target\release\fallout.exe expression-plans `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --bundle local/plans-new.bin --diagnose-structure `
  --output local/plans-diagnostic-new.json
```

The second command returns **1** for the pinned corpus. Diagnostic mode retains
every statement and reports three residual stacks instead of admitting them to
execution. Strict mode, without `--diagnose-structure`, rejects the first such
statement and publishes no report. These are findings under the selected model;
the original engine's handling is unmeasured, so they are not labelled repaired
content, original rejection behavior or an accepted compatibility rule.

| Installed authored corpus | Development comparison |
| --- | --- |
| Compiled bodies | 14,514 |
| Set/if/elseif expressions | 53,404 |
| Complete structural plans | 53,401 |
| Nodes in complete plans | 133,235 |
| Maximum stack / height | 6 / 14 |
| Retained residual-stack findings | 3, each with two operands |

All three findings belong to `FalloutNV.esm` INFO records. The final receipt keeps
the record/SCDA offsets, full source/body/expression/token hashes and structured
diagnostic. It publishes no raw script bytes. Rust uses a forward operand stack;
the separately owned C++ tool constructs trees backward using pending child slots.
Every source statement/hash, ordered shape tuple and finding agrees in the
development comparison. This remains a comparison of Rust-extracted SCDA, not a
claim that the C++ tool independently extracts plugins or executes retail scripts.

Nine Rust tests cover operand order/subtrees, exact and aggregate budgets, retained opaque
bytes, context consumption, malformed models/stacks/bundles, diagnostic retention
and nonrecursive 50,001-node plans. Six independent positive fixtures include that
deep chain. Nine structural negatives reject in strict mode and retain equal
diagnostics in both readers. Final committed proof also repeats the source form
list, query/condition, native save/item and content regression chain.

The CLI now gives its inspector worker an explicit 8 MiB stack. This fixes the
expanded debug command builder overflowing the Windows initial thread during
`--help`. The allowance belongs to tooling; it does not replace parser budgets or
alter canonical simulation storage. The checkpoint rebuilds the debug CLI and
records a real successful `--help` process.

Source references are the pinned
[ScriptAnalyzer structural/decompiler routines](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/ScriptAnalyzer.cpp)
and [GameScript operator declarations](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameScript.h).
Selected ranges and full-file hashes are recorded in `sources.lock.json`; no
upstream implementation is copied or linked.

Original arithmetic precision/conversion, tilde effects, short-circuit behavior,
native numeric returns, jump origins, event scheduling and script effects remain
unverified. The plan provides source structure for those later layers. It does
not initialize a VM, run commands, interpret GetItemCount lists or accept gameplay.
