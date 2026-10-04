# CELL lighting source declarations

`world::lighting::CellLightingSources::load(&mut RecordStore, &FormKey, Limits)`
prepares one strict winning NV CELL and its declared LTMP target. The private
Arc-backed request offers borrowed `receipt()`, `root()` and `identity()` values;
serialized reports cannot construct a request. `validate_sources()` verifies the
complete ordered protected plugin cohort before reuse by a future consumer.

The producer reuses `decode_cell`, master-context `dependency`, `FieldSite`, the
existing subrecord visitor and bounded record reads. It preserves the winning
header, canonical identity, source ordinal/plugin/hash, decoded body hash, CELL
DATA flags and exact XCLL/LTMP/LNAM declarations. Complete field framing includes
validated XXXX prefixes. Decoded header/payload/frame offsets stay distinct from
stored-body offsets; compressed fields have no claimed physical frame offset.

The pinned xEdit FNV CELL declaration permits XCLL lengths 28, 32, 36 and 40.
The first seven entries require 28 bytes; `SetOptionalFrom(7)` permits the final
three four-byte entries to be absent. LGTM DATA has the same entries without that
optional declaration and requires 40 bytes. Evidence is xEdit commit
`9fb016884bec138ea6c7b872cec831537d464c3e`, complete CELL/LGTM declarations in
`Core/wbDefinitionsFNV.pas`, `wbByteColors` in `Core/wbDefinitionsCommon.pas`, and
the struct optional-entry implementation in `Core/wbImplementation.pas`.
Editor `SetAfterLoad` fixups are excluded.

| Byte offset | Retained input |
| --- | --- |
| 0, 4, 8 | Ambient, directional and fog RGB bytes plus each unused fourth byte |
| 12, 16 | Fog near/far exact u32 float words |
| 20, 24 | Directional XY/Z rotation words and their signed i32 interpretation |
| 28, 32, 36 | Optional directional fade, fog clip distance and fog power words |

No float conversion discards NaN payloads, negative zero or infinity. Missing
tail entries remain `None`. LTMP and LNAM require four bytes and preserve their
raw u32 words, including unknown inheritance flags. LTMP resolves through the
winning CELL's own master table with expected kind LGTM. Null, missing, deleted,
wrong-kind and missing-DATA targets have explicit source statuses; deleted and
wrong-kind winners retain header evidence without reading their bodies. Missing
CELL fields remain absent. No older winner, parent or editor default fills them.

`source_inputs_available()` requires a present CELL XCLL or template DATA and no
unavailable declared template. This is input presence only. It does not evaluate
LNAM inheritance, choose effective light values or admit rendering/runtime work.

| Construction allowance | Fixed ceiling |
| --- | ---: |
| Ordered source plugins | 256 |
| Retained CELL/target identities | 2 |
| Visited fields | 65,536 |
| Each stored/decoded requested body | 4 MiB |
| Aggregate max(stored, decoded) requested bodies | 8 MiB |
| Retained raw field framing | 256 bytes |
| Conservative retained and temporary metadata | 16 MiB |
| Declared template links | 1 |

Limits may only be lowered. Admission precedes body reads, raw copies, key/name
copies and template work. The metadata estimate includes source receipts and
temporary CELL full-name decoding; it excludes the caller's existing index and
allocator overhead. Source fingerprinting reads are separate from requested-body
accounting. Unsupported sizes, duplicate selected fields, malformed/truncated
framing, recovered checksum-tainted bodies and exceeded budgets return no request.
Identity covers the cohort and exact retained declarations independently of the
chosen lower limits. `runtime_ready` is always false.

Eight independent literal tests cover every supported prefix, every color byte
and word, all optional absences, exact field spans, XXXX framing, compression,
master-context winning overrides, each template status, absent fields, NaN and
unknown flags, strict malformed/tainted refusal, changed names/order/count/bytes,
and exact/one-under admission for every allowance. These are engineering/source
checks; installed consumer evidence and original gameplay acceptance are separate.

`cell-lighting-sources --install ... --load-order ... --cell Origin.esm:hex`
is the owned headless consumer. An optional private `--index-cache` reuses the
existing protected header index. It opens no archives and schedules no jobs.
Reports retain ordered plugin/order fingerprints and the complete source receipt.
`source_request_prepared` indicates factory success; `source_inputs_available`
reports source presence separately. Explicit unavailable declarations retain a
receipt and status. Factory refusals retain a source error and null receipt.
The CLI emits its report and exits unsuccessfully for either unavailable or
refused inputs; available declarations exit successfully. No effective light,
inheritance evaluation, rendering or retail-parity result is emitted.
