# Compiled New Vegas script inspection

The Rust reader now frames real SCDA bodies without requiring SCTX source text.
This is a content-loading prerequisite for the script VM. It does not execute
commands, advance quests or accept a gameplay scenario.

The installed ten official plugins contain 14,514 compiled bodies and 1,966,273
compiled bytes. Initial inspection found 142,218 instruction headers, 28,463
reference calls and 5,702 event headers. There are 217 distinct top-level native
command IDs. Commands inside expressions are still opaque, so 217 is not the
complete command coverage requirement. Condition functions retain their separate
census and identifier space.

## Run the census

Build the CLI, then use fresh output paths:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli
target/release/fallout.exe scripts --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --output local/scripts-new.json
```

The focused scan strictly reads SCPT, INFO, QUST, PACK, PERK, TERM, REFR, ACHR,
ACRE, PGRE, PMIS and PBEA bodies. xEdit's pinned FNV definitions place standalone
or embedded scripts in these records. The report counts every other deferred
body; it does not certify its contents. On the supplied official corpus, focused
body counts and compiled-byte totals match the earlier full diagnostic census.
This comparison does not make its known malformed LAND body trustworthy.

Without `--defer-unrelated-payloads`, a full strict scan still stops at LAND
`00150FC0`, file offset `0x0B0CFF04`. No complete report is published after that
failure. The selected script bodies do not need checksum recovery.

Each body reports its source digest, record kind/ID, record file offset and
separate decoded offsets for the SCDA field header and its data. Instruction
offsets start at the first SCDA data byte. A compressed record's decoded offsets
are not file offsets. SCHR size/type/reference/variable metadata is retained;
missing or malformed headers and declared-size differences are explicit issues.
A header cannot be silently reused for a second compiled body.

## What the decoder knows

Instruction headers are little endian: an opcode and operand byte count, with
an extended reference-call prefix for opcode `0x1C`. The reference index belongs
to the script's reference list; it is not a FormID. An explicit zero remains
distinct from the absence of a prefix.

Known statement headers are named separately from native command IDs. A begin
statement exposes its two-byte event ID and four-byte end-jump field. The jump
field stays an authored number; its execution origin and target are not guessed.
Unknown headers preserve their extent and opaque bytes. They cannot be used as
executable instructions. The official corpus has no unknown headers under this
model, which says nothing about support for their operands or command behavior.

The decoder borrows operand slices, walks iteratively and checks every extent.
Default limits are 4 MiB and 262,144 instructions per body. Census reports also
limit each plugin to 262,144 bodies and 64 MiB of compiled data. Zero-length
instructions still consume a header and count against the instruction budget.

The source reference is [xNVSE at
0ccd23ad885ddae533c1790a3fc56cd073e38de3](https://github.com/xNVSE/NVSE/tree/0ccd23ad885ddae533c1790a3fc56cd073e38de3):
`ScriptAnalyzer.h` names statement values; `ScriptAnalyzer.cpp` implements
`ScriptIterator::ReadLine` and `BeginStatement`; `CommandTable.cpp::Init` separates
console/script command ranges. Both ScriptAnalyzer files were read completely.
[xEdit's pinned FNV schema](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
defines SCHR and the containing embedded-script records. These are format
references; neither upstream runtime was built or used for execution here.
xNVSE's per-component license audit remains incomplete. No upstream implementation
code is copied or linked into this decoder, and no fixed native address is used.

## Independent comparison

`tools/script-oracle` is an original, offline C++ header reader. It is not a
runtime dependency or an xNVSE build. Rust can export a local comparison bundle:

```powershell
target/release/fallout.exe scripts --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --defer-unrelated-payloads --comparison-bundle local/scripts-new.bin --output local/scripts-new.json
local/script-oracle-build/Release/script-oracle.exe local/scripts-new.bin
```

The original tool format `FROBS001` contains an eight-byte magic followed by
repeated four-byte body lengths and raw SCDA bytes. The bundle stays under
ignored `local/`; public receipts contain metadata and hashes. CMake builds the
oracle with MSVC and Windows CNG SHA-256. It independently frames each body and
computes a digest of every instruction boundary/opcode/caller/event tuple. Whole
body hashes cover the remaining opaque operand bytes. Exact matches cover all
14,514 bodies in the initial comparison.

The comparison starts at Rust-extracted SCDA. It does not independently certify
plugin decompression, subrecord extraction, reference resolution, operand layouts,
event scheduling or native behavior. Checkpoint tooling preserves that scope,
checks original installation fingerprints, runs the workspace checks and rejects
nine malformed original comparison bundles. Historical render evidence retains
its own tested revision; this script checkpoint does not rerun GPU acceptance.

Next work is a validated script bundle with local-variable/reference binding,
expression and native argument decoding, event/command registries and a reference
interpreter. Execution requires separate observed behavior evidence.
