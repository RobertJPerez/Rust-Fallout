# Source control-flow structure

The planner matches event and conditional delimiters in freshly decoded SCDA.
It retains ordered arms, enclosing ifs, final endifs and raw distance fields.
These are source observations. The API does not choose executable successors,
evaluate conditions, dispatch events or call native handlers.

The framing reference is the pinned xNVSE
[ScriptAnalyzer.cpp](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/ScriptAnalyzer.cpp):
selected `ScriptIterator::ReadLine`, `BeginStatement`, `ConditionalStatement`
and `ScriptAnalyzer::ParseLine` routines. The selected statement enum and field
declarations come from
[ScriptAnalyzer.h](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/ScriptAnalyzer.h).
Exact line ranges and whole-file hashes are recorded in `sources.lock.json`.
No upstream implementation is copied or linked.

The analyzer's conditional field name suggests a byte distance. Our corpus scan
instead observes an instruction-count relation for most conditional and else
fields. The new API calls this field `raw_word` and keeps the observed relation
explicit. It does not change the earlier lossless expression envelope or adopt
a retail jump origin from the field's name.

| Source relation checked | Observation required for a complete plan |
| --- | --- |
| Begin event | Raw u32 equals matched end instruction's end offset minus begin instruction's end offset |
| If / else-if / else | Raw u16 equals the number of framed instructions strictly between that arm and its next sibling arm or final endif |

The event span includes the end header. Conditional observations count reference
prefixes as part of their one framed instruction, regardless of byte length.
Original execution may handle the fields and delimiters differently. Matching
these relations is insufficient to admit a script to a VM.

`Plan` owns a private, newly decoded instruction list borrowed from the exact
input bytes. Callers receive immutable access to instructions, events, arms and
links. A supplied mutable `Program` cannot forge a plan for another buffer.
Event arguments, native operands and other statement operands remain source
bytes. Expression extent is checked, but token structure and semantics still
require their separate decoders and capability checks. Fragments without event
blocks are allowed; original INFO and quest fragments require this shape.

Rust pairs delimiters forward with open-group indices and explicit sibling
chains. The owned C++ oracle validates the same structural contract and pairs
delimiters backward from closing markers. Both paths use bounded iterative
construction, traversal and destruction. Every sibling is completed once, so
planning takes O(instructions) time and O(instructions) memory. No recursive
tree owns the nested blocks.

Defaults allow four MiB per SCDA body, 262,144 instructions per body and 65,536
conditional levels. The offline bundle additionally limits total bytes to
66 MiB, bodies to 65,536 and framed instructions to two million. The same
aggregate instruction budget includes unresolved bodies. A 30,000-level fixture
checks construction and drop without depending on process stack size.

Strict inspection returns an error before publishing a report for the first
unresolved body. Diagnostic inspection retains that body's hash, size,
instruction count and first structural finding, with `structure: null`; it
continues to inspect the remaining bodies and returns exit code 1. Malformed
bundle framing remains a hard error in both modes. No body is repaired or
partially admitted to execution.

The development comparison covers all 14,514 authored original SCDA bodies and
142,218 instruction headers. Rust and C++ agree on 14,461 complete structures
and 53 unresolved bodies: 37 first orphan-end-if findings, seven first orphan-arm
findings, four first arm-after-else findings and five raw-distance mismatches.
Complete bodies contain 5,592 events, 24,636 arms and 30,228 links; maximum depth
is ten. Counts of events and arms here describe complete bodies only. A first
finding is not an inventory of every irregularity in that body, nor evidence
that the original runtime rejects it.

Checkpoint evidence binds every body to the fresh extraction's plugin hash,
record kind/FormID, record file offset and decoded SCDA field/data offsets.
Those decoded offsets are not compressed-file offsets. The bundle does not
contain winning-script handles or live instances. Authored definitions and
winning runtime source versions remain distinct.

```powershell
.\target\release\fallout.exe control-flow --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --bundle local\YOUR-RUN\expressions.bin --diagnose-structure --output local\YOUR-NEW-REPORT.json
```

Original payloads remain in ignored local evidence. Public receipts contain
identities, hashes, counts and findings. Final workspace, independent fixture,
source-cohort and installation verification belongs to the immutable checkpoint
receipt. Runtime handling of all findings, branch truth, short-circuit effects,
event lifecycle, numeric coercion and native execution remain open.
