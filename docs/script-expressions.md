# Compiled expression envelopes and tokens

Rust now decodes vanilla `set`, `if` and `elseif` expression envelopes without
source text. The bounded tokenizer borrows original bytes, retains token ranges
and preserves command argument payloads. It does not evaluate expressions or
execute commands.

```powershell
target/release/fallout.exe expressions --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --output local/expressions-new.json
```

The same command can write a fresh local `--comparison-bundle`. The bundle is
ignored by Git and guarded against destinations inside the installation. The
focused scan leaves unrelated bodies explicitly deferred. Without that option,
the strict scan continues to reject the known installed LAND checksum mismatch.

## Format evidence

Pinned [xNVSE ScriptAnalyzer](https://github.com/xNVSE/NVSE/tree/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse)
documents assignment targets, conditional jump fields, expression lengths,
variable/reference tokens, quoted literals and embedded command envelopes.
The complete analyzer files were read previously; the relevant routines were
inspected again for this decoder. No upstream implementation is copied or linked.

The exact fingerprinted executable supplies all 16 operator descriptors, including
their spellings and precedence bytes. Original Rust and original C++ independently
read these file bytes. Addresses from the pinned `GameScript` declarations are
mapped through PE sections; executable code is never loaded or called. The
descriptor inspection from checkpoint 16 is freshly compared here after sharing
original offline C++ PE helpers.

Tokens retain local type bytes and indices, reference-table indices, explicit
context prefixes, command IDs, operator IDs and source ranges. Context prefixes
remain visible even when zero or left unconsumed. Variable/command context
consumption follows the inspected structure; it does not resolve a loaded script
or reference value. The parser retains trailing statement bytes instead of
inventing their meaning.

Quoted bytes remain uninterpreted, including NUL and non-ASCII bytes. The vanilla
expression reader explicitly leaves `n`/`z` meanings unknown, so those tokens
reject. Their layouts in native command arguments cannot be substituted into an
expression. Extension expression encodings need separate evidence and decoders.

Numeric tokens preserve their decimal text without floating-point conversion.
The independent reader uses the current CRT only to obtain a decimal token
boundary. [Microsoft's CRT documentation](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/strtod-strtod-l-wcstod-wcstod-l?view=msvc-170)
records differences between older CRT and UCRT parsing. This checkpoint therefore
does not claim original numeric values, rounding, locale behavior or evaluation.
Hexadecimal, infinity/NaN and historical extensions are outside the decoder's
explicit decimal subset.

## Corpus and independent comparison

All ten official source plugins produce the following observed metadata:

| Check | Count |
| --- | --- |
| Compiled bodies covered | 14,514 |
| Set/if/elseif envelopes | 53,404 |
| Expression bytes | 538,070 |
| Tokens | 149,082 |
| Embedded command calls / distinct IDs | 16,320 / 143 |
| Trailing statement bytes / pending contexts | 0 / 0 |
| Expression decoding issues | 0 |

All 143 embedded command IDs bind to descriptors in the original vanilla table.
Their argument payloads are still opaque and their handlers are unimplemented.
The counts cover these three expression statement kinds, not every possible
command-parser extension or inline expression inside native arguments.

`tools/expression-oracle` independently parses Rust-extracted SCDA, reads its
operator table directly from the original executable and compares every statement
envelope, token tuple digest and aggregate count. Raw SCDA hashes cover whitespace
and all opaque bytes. Plugin framing/extraction and decompression remain outside
this oracle's comparison, as do reference binding and behavior.

Each token contributes a fixed 58-byte little-endian tuple: u32 start/end, u8 tag,
u8 type byte, u16 index, u8 context presence, u16 context index, u32 operator ID,
u8 precedence, u16 command ID, u32 payload length and 32-byte payload SHA-256.
Unused slots are zero. Payload hashes apply to quoted strings, numeric lexemes
and opaque command arguments. Tags are local=1, global=2, reference literal=3,
context prefix=4, command=5, string=6, number=7 and operator=8.

No quoted expression token occurs in the supplied corpus. An original synthetic
literal fixture is compared separately, including NUL, non-ASCII and operator
bytes. Malformed lengths, unknown tokens, unsupported numeric forms and truncated
targets reject rather than create a successful partial expression. Independent
negative fixtures exercise those failures.

Next layers are native arguments, expression operand binding, postfix structure,
control flow and verified execution effects. Expression operators are currently
metadata; stack behavior, short-circuiting, precision and division errors remain
unmeasured. No quest, event lifecycle or retail/gameplay acceptance is established.
