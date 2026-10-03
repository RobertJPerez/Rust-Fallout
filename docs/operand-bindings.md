# Compiled operand associations

Rust now associates encoded local and reference indices with the tables belonging
to their authored SCHR unit. The binder covers instruction caller prefixes,
assignment targets, expression locals/globals/reference literals/context prefixes,
and native regular/message operands. It neither loads forms nor evaluates values.

```powershell
target/release/fallout.exe operand-bindings --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --output local/operand-bindings-new.json
```

The CLI writes a diagnostic report and returns one for the three known empty-unit
metadata inconsistencies. These are preserved source findings. All supplied
compiled units have no missing index associations or decode issues. A successful
table association is still separate from a valid loaded form or runtime value.
An optional local `--comparison-bundle` retains decoded records outside the
installation; public reports include only metadata and hashes.

Reference indices start at one. SCRV entries point to a declared local index,
while SCRO entries retain their raw source FormID. Sparse local indices are
looked up by their declared value, not by list position. Repeated declarations
remain in authored order; lookup selects the first declaration as described by
the inspected xNVSE lookup routine. Original retail handling of conflicting
duplicates remains unmeasured.

Foreign locals need special care. Their encoded reference context associates with
this script's table, but the local declaration belongs to the referenced script.
The binder retains that relationship and defers declaration/value resolution.
It never binds a foreign index to an equally numbered local in the current unit.
Local type bytes are retained without turning a declaration flag into an inferred
numeric/reference type.

Pinned [xNVSE ScriptAnalyzer](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/ScriptAnalyzer.cpp)
and [GameAPI](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameAPI.cpp)
provide the inspected context and operand structures. Existing script table
evidence comes from pinned xEdit and xNVSE. No upstream implementation is copied
or linked. This source research does not establish retail loading behavior.

| Observed association | Count |
| --- | --- |
| Compiled units | 14,514 |
| Encoded operand uses | 159,111 |
| Current local declarations | 26,888 |
| SCRO form associations | 102,895 |
| SCRV local associations | 3,768 |
| Foreign local declarations deferred | 25,560 |
| Missing associations / decode issues | 0 / 0 |

The original offline C++ `tools/operand-oracle` independently parses owning tables,
SCDA expressions and native operands from Rust-extracted decoded records. Every
compiled unit's metadata hash, SCDA hash, binding tuple digest and count matches
Rust. Extraction/decompression and loaded target lookup remain outside this
comparison. The refactored original C++ table and native helpers receive fresh
complete comparisons over their prior scopes, including expressions and original
executable descriptors.

Each use contributes a fixed 33-byte little-endian tuple: u32 SCDA index offset,
u8 role, u16 encoded index, u8 context presence/u16 index, u8 status,
u8 target presence/u32 value, u8 reference-field presence/u32 decoded offset,
u8 local-declaration presence/u32 decoded offset, u8 local-type presence/u8 byte,
u8 context target kind/u32 value. Absent slots are zero. Whole unit metadata and
SCDA hashes bind source bytes independently of the association tuples.

Roles are caller=1, assignment local=2/global=3, expression local=4/global=5,
reference literal=6, context prefix=7, argument local=8/global=9/form reference=10/
form variable=11/context prefix=12. Statuses are current local=1, form=2,
reference variable=3, deferred foreign local=4, missing local=5/reference=6/
reference variable declaration=7. Form zero remains a raw association; its loaded
null behavior is not inferred. Event operands and declaration statements are
outside this binder's current scope.

Missing entries stay explicit. Bytecode mismatch, malformed operands and use/unit
budgets reject. Original synthetic fixtures cover sparse indices, zero/missing
references, context isolation, form-variable association and malformed expression
coverage. No event, quest, native side effect or retail/gameplay gate is accepted.
