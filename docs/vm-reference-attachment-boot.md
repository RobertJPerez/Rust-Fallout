# Explicit boot of an existing placed reference

VM28 joins one existing canonical reference to its exact authored script and
initializes that script in a private result snapshot. This is an opt-in
engineering host operation. Original activation, script replacement, event-list
lifetime and gameplay parity remain unverified; Faithful returns a refusal.

The first admitted source chain is a winning, nondeleted REFR with a unique NAME,
a winning, nondeleted CONT with exactly one four-byte, non-null SCRI, and a
winning, nondeleted SCPT containing exactly one schema-verified standalone loaded
unit without source findings. Actor inheritance and other base layouts remain
unavailable. The existing placement decoder, subrecord visitor, form resolver,
indexed catalogue and prepared-source cache supply these facts.

The pinned xEdit revision is `9fb016884bec138ea6c7b872cec831537d464c3e`.
`Core/wbDefinitionsFNV.pas` has SHA-256
`89b1420415b858e004429e89a828348a1df71f5c8b199d4ea66e02f0ca41b012`;
`Core/wbDefinitionsCommon.pas` has SHA-256
`e616b6546f6df74d88ec98bb870906db380c985963d011e4931c5726682b7d26`.
Complete consulted definitions: CONT 4201–4227, SCRI/script metadata 3022–3074,
SCPT 4919–4931, REFR 7461–7783, record header 8565–8585. CONT's SCRI layout is
unconditional. The header preserves a raw u16 FormVersion without an accepted
value constraint; this adapter preserves that value and does not fabricate a
numeric whitelist. Unsupported kinds or SCRI widths refuse.

`script_reference_attachment::request` alone constructs the sealed request. Its
fields are private and it has no deserializer. A borrowed proof is diagnostic:
editing a separately serialized proof cannot create attachment authority. The
reader checks the complete ordered source receipts, full winning-header digest,
every retained script's original source SHA, and the selected script's fresh
decoded body, source, header offset, flags and exact loaded handle. Relabeling a
stale catalogue's public receipts cannot make its retained script current.

The source proof retains complete headers and hashes for placement, base and
script, plus NAME/SCRI offsets, raw words and resolved keys. Decoded offsets follow
the existing visitor: they identify subrecord headers, not field data bytes.
Field preflight reserves the placement decoder's visits and unknown-field name
bytes before that decoder runs. Source counts, aggregate file hash bytes, header
visits, catalogue visits, variable strings, stored/decoded record reads and field
visits have explicit bounds. `preflight` returns diagnostic counts only and lets
the CLI admit source/hash/header work before catalogue loading. The sealed reader
repeats it. These are logical payload/work bounds, not allocator accounting.

`reference_attachment_boot::prepare` consumes the sealed request and current
PreparedSources, a precise reference/key selection, explicit intent and the same
initialization request used by quest boot. Shared crate-private VM21 helpers
retain quest validation order, limits, counters and mutation behavior. No new
runtime storage, canonical transaction or snapshot schema is introduced.

`BootPlan::apply` consumes a strict current Snapshot, restores a private World,
and requires both `reference_origin(id) == Some(expected_key)` and
`authored_reference(expected_key) == Some(id)`. Dynamic, missing, mismatched or
already-owned selections refuse. It uses existing `create_instance` and `assign`
for `Owner::Placed { reference: id }`. Context roles and local values are supplied
explicitly; unset locals retain canonical Uninitialized. Late context, local or
capacity failures discard the entire private World. No reference, event,
inventory or pose is created or changed by this route.

The saved consumer is `event-operands --reference-boot-request REQUEST
--snapshot-input INPUT --snapshot-output RESULT [--output REPORT]`, with the
existing installation and load-order arguments. REQUEST schema 1 requires the
reference ID, expected authored key, intent, initialization, all eight
`source_limits`, all five `initialization_limits`, source instruction/operand/token
limits and trace/result/report byte limits. Context requires explicit nullable
calling_reference, containing_reference and target fields, and an explicit
arguments array. Unknown, duplicate or missing request fields refuse. The new
path option preserves OS path parsing through its boxed value.

Requests are at most 16 KiB; inputs are strict current schema 4 and at most 64 MiB.
Caller limits can tighten the fixed ceilings. Catalogue loading uses tightened
existing script/record/read/retention limits; selected preparation uses the named
instruction, token and operand bounds. Catalogue reads and the subsequent fresh
three-record verification are separate bounded passes: the authored catalogue
reads SCPT205 plus two REFR53 bodies (311 bytes), while the selected chain reads
SCPT205, CONT31 and REFR53 (289 bytes). Compact borrowed trace
admission precedes initialization. Result encoding precedes complete cold
restoration and equality checks. The complete pretty report, including its final
newline, is admitted before fresh output creation. Result/report must be distinct
and outside the protected installation; common create_new writers preserve
existing artifacts. An interrupted filesystem write remains incomplete.

The independent authored fixture contains two existing references sharing one
CONT, an unrelated placed script owner, pending event, inventory count 19, NaN
condition bits and opaque bytes. Expected whole snapshots explicitly add only the
selected Owner::Placed instance, initialized local bits/context and canonical
instance/revision changes. Tests cover literal NAME 13/SCRI 10/SCHR 15 offsets,
SCPT file offset 42 and raw header version 65535, cold identity, exact/one-under
limits, source-chain refusals, relabeled stale receipts, late initialization
failure and unchanged original fixture bytes. Five new runtime tests and four
quest regression tests pass. The frozen CLI passed 347 actual cases: 125 new
reference attachment, 41 quest boot, 75 reference copy, 60 foreign copy, 16 numeric
copy and 30 native observation. Build, CLI/runtime Clippy and formatting checks
pass. The fixture also preserves unrelated pose bits (negative zero and
subnormal), enabled state, inventory and pending order; selected reference ID
`9007199254741099` remains an exact typed integer. Local receipts retain the
initial lifetime compile failure, corrected field-offset fixture expectation and
corrected catalogue-vs-chain read-budget expectation. Frozen producer SHA-256:
`49fef715ceeed12fb98806234a564813eba185eaf3c9d6b3a06b3c0f2e73a524`.
