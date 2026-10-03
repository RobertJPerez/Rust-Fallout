# Native command operands

The Rust script layer now decodes vanilla native operands in instruction calls
and commands embedded in set/if/elseif expressions. It keeps source ranges,
parameter IDs and exact operand bits. This is a reusable data reader; native
handlers, numeric conversions and loaded reference values remain unfinished.

```powershell
target/release/fallout.exe native-arguments --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --output local/native-arguments-new.json
```

An optional `--comparison-bundle` writes fresh Rust-extracted SCDA to an ignored
local destination outside the source installation. Without the focused option,
unrelated bodies are validated strictly and the known LAND checksum defect still
rejects. Original assets are never repaired or included in public reports.

The pinned [xNVSE GameAPI](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameAPI.cpp)
supplies operand classification and variable/numeric/form extraction layouts.
[ScriptAnalyzer](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/ScriptAnalyzer.cpp)
supplies parsing-convention and message-substitution structure. These selected
routines were inspected as format evidence; no upstream implementation is copied
or linked. Licensing for the inspected xNVSE files remains incompletely audited.

Both readers obtain signatures and parse-handler metadata from the exact original
executable fingerprint already required by the command catalogue. The handler
address is used only to select a verified operand layout. No original code runs.
Optional words zero and one are supported explicitly; other bits reject.

Strings retain raw bytes, including NUL and non-ASCII values. Integer operands
retain signed 32-bit values; doubles retain all 64 bits, including nonfinite
patterns. A numeric variable can include a reference context. Form operands
retain either a reference-table index or a three-byte local-variable encoding.
That local encoding follows GameAPI's extraction layout; it does not reproduce
the different cursor handling in the inspected decompiler helper.

Absent payloads and explicit zero argument counts remain distinct. Message
substitutions are bounded to nine. Unknown conventions, unsupported parameter
classes, compiler overrides, inline extension expressions and truncated operands
reject. Byte, parameter, string, body and call limits apply before report growth.
Errors remain visible; they do not produce invented successful bindings.

All ten official plugins were inspected:

| Check | Count |
| --- | --- |
| Compiled bodies compared | 14,514 |
| Instruction / expression calls | 51,611 / 16,320 |
| Distinct native command IDs | 355 |
| Regular / message substitution operands | 80,377 / 75 |
| Native payload bytes | 465,675 |
| Decode issues | 0 |
| Preserved trailing bytes / calls | 6,868 / 1,717 |

Every trailing operand belongs to ShowMessage and has four bytes. Their semantics
are unverified. They remain present in the source hash and separate trailing
digest; a matching byte extent does not certify message behavior. No form-variable
operand occurs in the official corpus; an original synthetic fixture exercises
that encoding independently.

The original offline C++ `tools/argument-oracle` independently reads the original
executable and parses Rust-extracted SCDA. It compares every call's source extent,
context, operand hash, counts, typed tuple hashes and trailing hash. This does not
independently verify plugin extraction/decompression. The shared original C++
expression reader is rechecked over its complete checkpoint 18 scope after the
refactor, and the command catalogue is compared freshly too.

Each operand contributes a 63-byte little-endian tuple: u32 start/end, u32 parameter
type, u8 value tag/type byte, u16 index, u8 context presence, u16 context index,
u64 numeric bits, u32 string length and 32-byte raw string SHA-256. Unused slots
are zero; message substitutions use u32::MAX for the absent parameter type.
Tags are string=1, integer=2, double bits=3, short=4, byte=5, global=6,
variable=7, form reference=8 and form variable=9. Signed integers contribute
their exact 32-bit representation zero-extended into the numeric slot.

Reference resolution, actual native parameter type checks, expression evaluation,
control flow, event operands, scheduling and side effects are next. No quest or
retail gameplay gate is accepted by this metadata comparison.
