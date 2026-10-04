# One explicit source event on an existing saved owner

VM29 appends one caller-selected source block to an existing script instance in
a private result snapshot. It is an Engineering host input, with no execution,
acknowledgment, clock advancement, inferred object-mask dispatch or retail
scheduling. Faithful returns a typed refusal because original dispatch and
event-list lifetime remain unverified.

`event_request::prepare` takes a read-only current World, current PreparedSources
and Content, an exact Selection, explicit Context and Limits. Selection requires
the persistent InstanceId, expected Owner, exact loaded Handle, BEGIN SCDA offset,
descriptor ID and intent. The existing cached plan's `event_at_scda_offset` proves
the complete selected source span. No first-block selection, second parser or
caller-imported report grants authority.

Only this constructor can return the private Request. It borrows immutable source
and bounded explicit inputs; it has no deserializer or mutable live World access.
Its trace preserves campaign, revision, current integer clocks, pending count,
source version/cohort/decoder, ordered receipts, exact source span and all context
roles. Complete source names, owner/definition strings and Content reference
strings are charged before canonical name validation or trace retention. A
compact borrowed trace is admitted before returning the request.

`Request::apply` consumes a strict current Snapshot, requires the observed
campaign/revision/clocks/pending count, restores a private World and rechecks its
complete cohort and exact current instance/owner/definition. Existing
`World::enqueue` validates the source trigger, references, context, pending
capacity and counters. It appends one event at the unchanged current clocks,
advances the canonical sequence/revision, and returns the complete result.
Failures discard the private World. This is a private saved-state operation;
the trace is historical diagnostic data, not a live mutation token or a claim
that unrelated fields were globally fingerprinted during preparation.

Source and canonical API evidence is the existing `events`, `World::enqueue`,
`validate_trigger`, `runtime_definition`, immutable `control_flow::Plan` and
`definition_plan::Plan::event_at_scda_offset`. Their implementation was read
before the new adapter. Descriptor IDs in BEGIN headers remain distinct from
ObjectEvent masks; the latter are not admitted by this request.

The actual saved consumer is:

```text
event-operands --install INSTALL --load-order ORDER
  --snapshot-event-request REQUEST --snapshot-input INPUT
  --snapshot-output RESULT [--output REPORT]
```

REQUEST schema 1 requires `instance`, `owner`, `definition`, `begin_scda_offset`,
`event_id`, `intent`, `context`, all six creation limits, four preparation limits
and result/report limits. Context requires explicitly present nullable
`calling_reference`, `containing_reference`, `target`, and an `arguments` array.
The private conversion helper also serves the old reference-boot route without
changing its flags, defaults or output. Missing, duplicate and unknown fields
refuse; no initializers, activation defaults or masks are accepted here.

Creation defaults are 1 MiB compiled source, 4096 selected-event instructions, 64
context arguments, 1 MiB logical variable strings, 256 receipt rows and 2 MiB
compact trace. Preparation of the selected whole definition is separately bounded
by `maximum_prepared_instructions`, `maximum_prepared_operand_uses`,
`maximum_prepared_tokens` and `maximum_prepared_record_bytes`. The ordinary
catalogue/content loaders retain their existing fixed limits. The selected
compiled source byte ceiling is checked before cache preparation. Creation
instruction counts cover the selected event, while preparation counts cover the
whole selected definition: the fixture has two events, four prepared instructions
and two selected instructions. Caller limits tighten the fixed ceilings.

Requests are at most 16 KiB. Inputs use current schema 4 and the existing 64 MiB
strict snapshot bound. A result must fit its explicit bound, cold-restore and
equal the complete private result. The complete pretty report, including its
final newline, must fit its explicit ceiling of at most 8 MiB before output
creation. Common fresh/protected/distinct output checks and create_new writers
preserve existing artifacts. An interrupted filesystem write stays incomplete.
Logical work/payload limits do not claim allocator accounting.

Three runtime tests and 452 actual CLI cases pass: 105 new event-request cases
and 347 old reference boot, quest boot, reference copy, foreign copy, numeric copy
and native observation cases. The authored fixture selects instance 2, owner
Placed reference 2, BEGIN offset 14, descriptor 5 and sequence 2. Caller 1,
containing reference 2 and target 3 stay distinct. The new row arrives at tick 17,
game 123, menu 7 and real 999 nanoseconds; no clock is advanced. Whole expected
and cold snapshots preserve prior queue order, other owners/locals, NaN and signed
zero numeric bits, inventory 19 with opaque/NaN condition data, and pose negative
zero/subnormal/enable state. Quest and Fragment owners also pass.

Instance and sequence `9007199254741099` remain exact typed integers. All six
creation limits, nonzero preparation counts and result/report counts have exact
and one-under tests; zero-token/operand preparation is accepted. Wrong source
identity, owner, BEGIN offset, descriptor, context references, stale schema/cohort,
pending capacity, exhausted sequence/revision, malformed/oversized requests,
conflicting flags and protected/existing/aliased outputs refuse without a result.
Original fixture files remain unchanged. Build, CLI/runtime Clippy and formatting
checks pass. The initial cloned-handle lint and old monolithic command enum size
failure are preserved in local receipts; the clone was removed and only the
declared foreign-copy optional path was boxed with its identical OS parser.

Frozen replacement producer SHA-256:
`efa9dd8a430bc5e01ca40fdb7d43dc70e7516a5f5bc5debd7318159672bad2db`.
The authored executable metadata was inspected only. Original installations,
saves/settings and research pins were untouched. Source and engineering evidence
do not satisfy the original semantic or gameplay gates.
