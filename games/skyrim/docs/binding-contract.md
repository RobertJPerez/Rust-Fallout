# Skyrim binding handoff

`skyrim-prep export-bindings --plugin <file> --output <new.jsonl>` exports source
declarations for a future shared Papyrus linker. It does not execute scripts,
select an active load order or resolve an override winner. Retail exports remain
under ignored `local/`; the Rust AST and synthetic tests are distributable source.

The first JSONL row identifies the plugin by filename, byte count and SHA-256.
Each `binding` row represents one physical VMAD subrecord. The last `complete`
row reports binding, decoded-tail and failure counts. Missing trailers and any
failure must prevent acceptance. A file output is published atomically without
overwriting an earlier result. Standard output may be partial on an I/O failure.
The matching census plugin entry supplies the master list; join by source SHA-256,
not by filename alone. Export failures count decoding failures, while the census
also reports master-index and dependency findings.

| Field | Meaning |
| --- | --- |
| `form_id` | Original 32-bit file-local record identity; not a runtime FE slot |
| `record_offset` | Physical record-header file offset |
| `subrecord_offset_in_decoded_record` | VMAD subrecord header within the decompressed record body |
| `vmad_offset_in_decoded_record` | VMAD data start in that body, after the six-byte subrecord header |
| `vmad_sha256` / `vmad_bytes` | Original VMAD data digest and size |
| Nested `offset` fields | Byte offsets from the start of VMAD data |
| `scripts` | Ordered primary attachments, including removed/status-marked entries |
| `tail` | `Absent`, `Decoded`, or `Unsupported` with original bytes and diagnostic |

Strings in the Rust contract are original byte arrays. Properties retain names,
status bytes, signed integers, float bits, raw boolean bytes, object references,
signed alias IDs, unused object bytes and ordered arrays. No UTF-8 conversion,
float canonicalization or boolean normalization is implied by the Rust API.

Supported tails are INFO/PACK event fragments, PERK fragments, QUST stage/log-entry
fragments and alias scripts, and SCEN event/phase fragments. Source order is
preserved. Selector fields follow xEdit's packed byte widths. For example,
Mutagen splits QUST's four stage bytes into a 16-bit stage and 16 unknown bits;
the comparison repacks them without claiming that all 32 bits are a stage number.
SCEN's packed index/unknown fields are similarly reconciled byte for byte.
Extra-bind versions other than 2, unknown event flag bits, VMAD versions outside
4/5 and mixed outer/alias object formats stay unsupported.

Fragment `file_name` metadata alone does not establish an executable binding.
Only actual fragment script names and nonremoved primary/alias attachments enter
required script-asset lookup. An available PEX does not establish that the named
class, property or function exists. Multiple file candidates remain ambiguous
until active load order and archive mounting precedence are implemented.

The independent checker reconstructs file-local IDs from Mutagen's FormKeys.
One installed Dawnguard object reference does not round-trip: ACTI `02014760`,
script `DLC1VampireLordBlockerSCRIPT`, property `Collision`, contains `0307B5B9`.
The plugin has two masters; the independent reader falls back to the current
plugin and returns `0207B5B9`. A direct read at physical offset 786615 confirmed
bytes `B9 B5 07 03` in this exact uncompressed source record. The Rust output keeps
those bytes and the census reports the out-of-range index. This is an open runtime
interpretation question, and the strict oracle intentionally exits 2.

Before runtime consumption: review a shared Skyrim identity namespace, add a
tested Skyrim PEX dialect through the existing Papyrus work, resolve active source
records and script candidates, link declarations/functions/properties, then test
native events and original-game quest behavior. A clean decode is not acceptance
of any of those steps.
