# Implementation checkpoint 15: compiled New Vegas script headers

The Rust content reader now frames authored SCDA bodies without needing source
text. Each instruction preserves its source extent and opaque operand bytes;
reference-list indices and begin event IDs keep their separate meanings.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 142 passing tests and Clippy with warnings denied |
| Sources | 116 source/tool/configuration files match implementation commit 69dc6ea |
| Compiled corpus | All 14,514 bodies and 1,966,273 bytes across ten official plugins |
| Independent headers | All 142,218 instruction tuples and body hashes match original C++ |
| Reference/event headers | 28,463 reference calls and 5,702 begin event headers |
| Command census | 217 distinct top-level native IDs; expression calls remain uncounted |
| Failure behavior | Nine malformed oracle bundles reject; full strict LAND failure remains |
| Source safety | All 464 installed files and 9,907,238,722 bytes still match baseline |

The focused scan validates twelve script-bearing record kinds and explicitly
defers 162,712 unrelated record bodies. Its body counts and byte totals agree with
the earlier full diagnostic census. That does not validate the deferred bodies or
resolve the known malformed LAND checksum.

The offline comparison starts at Rust-extracted SCDA, so plugin decompression,
field extraction and reference linking are outside its independent scope.
Operands, expression calls, jump execution, native behavior and scheduling remain
unfinished. No script, quest, retail or gameplay scenario is accepted.

See [compiled script inspection](../docs/compiled-scripts.md),
[the scoped receipt](checkpoint-15-compiled-scripts.json) and
[verification](checkpoint-15-verification.json). Historical terrain/GPU/collision
receipts retain their original tested revisions. The terrain viewer received
positive user visual feedback; matching retail cameras and behavior remain open.

Next work follows the brief's dependencies: script metadata/reference binding,
command/event signatures, expression decoding and an evidence-backed interpreter,
alongside the remaining M1 format and asset-loading work.
