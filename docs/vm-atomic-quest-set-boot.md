# Atomic explicit quest owner boot

V3-VM-33 extends the existing single quest attachment boot with one private,
bounded quest-set result. `event-operands --quest-boot-set-request request.json
--snapshot-input before.json --snapshot-output after.json --output report.json`
creates all explicitly selected engineering owners or publishes no result.
This is storage initialization, not ObScript execution or retail quest activation.

`attachment_boot::prepare_many` borrows `PreparedSources`, `Attachments`,
`Content`, and `Selection { quest, expected_definition, initialization }` rows.
It accepts the existing `local_copy::Intent`. Faithful returns the existing
unverified-retail-semantics outcome before creation. Engineering returns a private
`ManyBootPlan`; its fields cannot be deserialized or edited.

The plan reuses `prepare_input`, `prepare_definition`, and the attachment checks
extracted from the single-quest path. The old request, limits, counters, validation
order, errors, output, and default CLI route remain unchanged. Both routes require
one winning non-deleted QUST, one checked SCRI, one exact standalone definition,
no retained attachment findings, and the complete ordered source cohort. Each
batch row additionally names the exact expected definition/version. One empty
validation World supplies content validation; it creates no state.

All selected rows must have the same campaign. Duplicate quest keys refuse.
Two distinct quests sharing one script retain separate owners, contexts and
locals. Only explicitly supplied row order determines instance allocation.
The CLI deduplicates prepared script keys while retaining quest selection order.
It loads one full source catalogue and one immutable selected source cache.
The existing prepared-source catalogue ceiling remains independent of the quest
root ceiling; unique selected definitions cannot exceed the admitted quest count.
Cached definition admission uses the existing plans; canonical instance creation
still builds its existing source schema/event-block view. No new decoder exists.

`ManyBootPlan::apply` consumes a snapshot and restores it once into a private
World. It validates source/content cohort, then calls the existing canonical
`create_instance` and `assign` through `initialize` for each row. Empty initializer
lists preserve canonical uninitialized locals and do not add an assignment
revision. Any late owner/context/type/reference/capacity/revision/allocator error
drops the private World and every preceding row. No partial `ManyBootResult`
escapes. Applying a plan again to its successful result refuses duplicate owners.
The operation never enqueues or acknowledges events, changes clocks or inventory,
creates references, infers stages, or chooses a retail activation schedule.

`ManyLimits` defaults to 32 quests, the existing five input ceilings, and 2 MiB
of selected variable-copy reservations. Initializers, context arguments, input
name bytes, receipt comparison bytes and declarations accumulate across rows;
shared definitions and repeated receipt comparisons are charged for each row's
work. Expected-handle names/digests are admitted before comparison. Each row
reserves six times its input name bytes, eight times its actual definition handle
string bytes, and 1024 metadata bytes before any private creation. These are
conservative logical copy reservations, not a peak-heap measurement. Canonical
World and current snapshot limits separately bound all pre-existing state.

The saved request uses strict schema 1 and at most 64 KiB. Every field is required:

- String-only `intent`: `engineering` or `faithful`; exact lowercase
  `source_cohort_sha256`; `selections` containing `quest`, `expected_definition`,
  and `initialization` with `campaign`, `context`, and `initializers`.
- Each context explicitly supplies nullable `calling_reference`,
  `containing_reference`, `target`, and an `arguments` array. Values retain their
  existing tagged exact integer/bit/reference representations.
- `maximum_quests`, all five fields in `initialization_limits`,
  `maximum_retained_variable_bytes`, `maximum_prepared_instructions`,
  `maximum_prepared_operand_uses`, `maximum_prepared_tokens`,
  `maximum_prepared_record_bytes`, `maximum_trace_bytes`,
  `maximum_result_snapshot_bytes`, and `maximum_report_bytes`.

Runtime/preparation ceilings can only be reduced. The trace ceiling is
2 MiB, report 8 MiB including its final newline, and result the current canonical
snapshot ceiling. Trace/result/report ceilings must be positive. Zero other
ceilings have their literal meaning. The borrowed typed trace retains each
physical QUST/SCRI/script receipt and explicit initialization. Trace admission
precedes private application; complete report admission precedes output creation.
Unknown, duplicate or missing fields, malformed requests, conflicting modes,
stale source/version/campaign/current snapshot and exhausted bounds refuse.

The CLI encodes and cold-restores the complete result, compares the entire
snapshot and every selected owner/definition, and then writes a fresh protected
output using the existing host helper. Input snapshot and installation remain
untouched. A Faithful request can emit an unsupported diagnostic and exits
unsuccessfully with no result artifact. Interrupted output is incomplete.
Synchronous source loading and bounded indivisible canonical calls do not promise
hard deadlines or responsive whole-installation boot.

Independent authored probes use two quests attached to SCPT 300 and a third to
SCPT 305, with exact negative-zero/NaN/reference initialization, preserved clocks,
an existing unrelated owner/pending event/inventory and full cold snapshot equality.
The expected complete snapshot is constructed directly from known schema/row data,
without invoking the batch or repeated single-quest initializer. Physical offsets
and QUST payload digests are independently calculated from authored byte assembly.
Runtime and frozen actual CLI probes cover order reversal, exact/under aggregate
bounds, late failures, strict input/output admission, and the old single-quest route.
These are engineering checks only. Original execution, gameplay, activation,
semantic conversion and retail parity remain unverified.
