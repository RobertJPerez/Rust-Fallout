# Authored script tables and caller associations

The Rust decoder now keeps each `SCHR` script unit separate inside its containing
record. It borrows the exact decoded fields, preserves authored table order and
does not require `SCTX` source text. This is metadata and operand groundwork,
not script execution or a playable quest implementation.

```powershell
target/release/fallout.exe script-bindings --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --output local/bindings-new.json
```

The focused scan explicitly defers unrelated bodies. A full strict scan still
rejects the known installed LAND checksum mismatch; this command does not recover
or repair that body. Source files are held read-only while hashing and scanning.
The current corpus produces a complete diagnostic report and exit code one
because three empty script units contain stale reference counts.

## Source layout and lookup

Pinned [xEdit FNV definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
describe the 20-byte `SCHR`, 24-byte `SLSD`, terminated `SCVR` name and ordered
`SCRO`/`SCRV` reference union. Every unknown or unused byte stays available in the
borrowed fields. Local names retain their original byte encoding. Header type and
flags stay authored; xEdit's embedded-script editing fixup is not applied.

Pinned [xNVSE GameScript.cpp](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameScript.cpp)
shows that reference-table lookups start at one and local-variable lookup searches
the declared index. Zero does not select the first reference. A form table entry
is an on-disk FormID interpreted through that source plugin's master list. A local
entry names a declaration in this script unit; it is not a form or a loaded value.

The decoder retains repeated declarations and indexes the first matching local,
following the inspected lookup. Eleven repeated indices occur in the supplied
base-game definitions; eight repeat a different name or declaration payload.
Both versions remain available. Original retail loading and duplicate-variable
behavior still need observation. The source-reference lookup alone does not
establish that behavior.

`SCHR` variable count is retained without imposing a guessed invariant. Counts
equal declaration length in 79,783 of 80,736 units and maximum declared index
(zero when absent) in 80,349. Neither equality is universal. Empty headers in the
official packs also retain nonzero counts with no declarations.

## Whole-source inspection

Across the ten official plugins, the focused scan observes:

| Authored metadata | Count |
| --- | --- |
| Script units, including empty headers | 80,736 |
| Compiled bodies / bytes | 14,514 / 1,966,273 |
| Source text fields, hashed but never needed for binding | 14,904 |
| Local declarations | 11,234 |
| Form / local reference table entries | 49,074 / 2,031 |
| Top-level caller references | 28,463 |
| Callers selecting forms / local declarations | 26,139 / 2,324 |

All top-level callers select an existing entry in their own table. Expression
references and native arguments are outside this checkpoint. Converting a raw
form to a stable source identity does not prove that the form exists, wins its
override chain or is a valid command subject. An embedded unit's record/field
offset is diagnostic provenance, not a persistent gameplay identity.

Three units without SCDA have reference-count mismatches: DeadMoney TERM
`0100923D`, FalloutNV INFO `0015AD0D`, and OldWorldBlues TERM `01004902`.
These are reported as original source inconsistencies, with raw counts preserved.
No compiled unit has a count, framing or caller-binding issue in this inspection.
No default reference or fabricated body is supplied for the empty units.

## Independent comparison

`tools/binding-oracle` is original offline C++ tooling. It parses complete decoded
record bodies supplied by Rust, including `XXXX`, script ownership, table fields,
local pairing, duplicate declarations and compiled caller prefixes. Every unit's
metadata fields, complete raw-field digest, caller counts and binding digest match
Rust. Malformed comparison bundles reject without a complete report.

The comparison begins after Rust plugin framing and decompression. It does not
independently compare extraction, stable-identity conversion, winning embedded
scripts, loaded variable values, expression/native operands or runtime behavior.
Those limits are explicit in the receipt. Existing full-census counts and the
checkpoint 15 framing receipt provide separate cross-checks of source coverage.

The raw local bundle is `FRUNIT01`, followed by records containing kind[4],
u32 FormID, u64 source file offset, u32 payload length and complete decoded bytes.
It stays outside the installation and is ignored by Git. Public reports contain
only provenance, counts and hashes, including the three original inconsistencies.

Metadata digests cover each script field's signature[4], u32 decoded header
offset, u32 data length and raw data, in authored order. Caller digests cover each
reference-prefixed instruction's u32 SCDA start, u16 table index, u8 target kind
(form=0/local=1) and u32 raw form or local index. All integers use little endian.
These recipes compare associations without serializing original source text.

Next work: expression references, native argument decoding, typed execution
boundaries, control flow and separately measured command/event behavior. Runtime
execution and retail parity remain false.
