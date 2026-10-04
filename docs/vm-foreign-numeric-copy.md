# Explicit engineering foreign numeric copy

`execution::foreign_copy::stage` admits exactly one existing journal head whose
source window is Begin, one Assignment, End. Its destination must be an own
numeric declaration. Its expression must be the existing two-token source
reference prefix and one numeric local read consuming that same prefix. The
three compiled operand associations must match their exact physical SCDA offsets,
reference-table field and declaration. Literals, operators, calls, globals,
foreign destinations and additional statements remain unsupported. Faithful
intent retains a typed refusal because original conversion and event lifetime
have not been measured.

The producer reuses `probe_event_operands_with_sources`, `foreign_target` and
`read_foreign`. The current canonical Quest or Placed owner selects the existing
foreign instance and its current declaration/value. Authored attachments do not
substitute an instance. Player dependencies require the supplied explicit role.
An initialized numeric `Value::Number` moves by its exact stored u64 bits into
the own destination; negative zero and NaN payloads have no conversion. The
foreign instance receives no writes.

The private stage contains the existing opaque own-instance `StagedEventChanges`
and an owned diagnostic trace. Its consuming commit uses
`World::commit_event_changes`, assigning one own slot and acknowledging one head
with one revision increment. Intervening revisions, foreign recycling, another
campaign or cold-restored epoch reject the old token before mutation. A trace
has no constructor/deserializer that can authorize a write.

Creation ceilings cover event instructions, three operand uses, statement bytes,
the existing event observation's source bytes/rows/UTF-8 strings/full binding
table, source metadata rows, transient probe strings and the complete compact
JSON trace. Canonical plugin names have no maximum length. Before any canonical
resolver/probe copies a context payload, the adapter measures borrowed names
from the bounded complete source receipts, dynamic context and supplied player's
authored origin. Its conservative reservation is twice the frame's variable
bytes plus seven times the largest measured name plus 1024 fixed bytes. The
existing probe temporarily holds its own target and the read target, each with
three possible names, alongside the prefix value. This is
a logical UTF-8 payload bound, not allocator accounting or another resolver.
Nonnumeric foreign declarations refuse before the general probe can clone a
possibly large reference value. Diagnostic trace serialization is admitted
before staging.

The separate saved consumer is:

```text
fallout event-operands --install COPY --load-order ORDER.json
  --snapshot-foreign-copy-request REQUEST.json --snapshot-input INPUT.json
  --snapshot-output RESULT.json --output REPORT.json
```

The strict schema-1 request names nonzero `sequence`, exact canonical `owner`,
`intent` (`engineering` or `faithful`) and required-nullable `explicit_player`.
All work/projection/metadata/probe/trace/result/report caps are explicit. Requests
are at most 16 KiB; caps may reduce defaults but cannot enlarge ceilings. The
input is a strict current snapshot, with no migrations or engineering seeding.
Only the head's exact existing definition is source-prepared. The private result
must pass complete encode/decode/cold-restore equality before publication. The
result is capped at 64 MiB and the complete pretty report plus newline at 8 MiB.
Both outputs must be fresh, outside the protected installation, and distinct
under Windows ASCII case folding. The report uses the shared writer. Strict
source/identity/capacity failures create neither output; typed unsupported
behavior emits a bounded refusal report, no result, and a failing CLI status.

For an existing Fragment activation 1 with journal head sequence 1, the request
can use these ceilings (the owner and sequence must match the actual input):

```json
{
  "schema_version": 1,
  "sequence": 1,
  "owner": { "kind": "fragment", "activation": 1 },
  "intent": "engineering",
  "explicit_player": null,
  "maximum_source_instructions": 4096,
  "maximum_operand_uses": 3,
  "maximum_statement_bytes": 65539,
  "maximum_trace_source_bytes": 1048576,
  "maximum_trace_rows": 65536,
  "maximum_trace_variable_bytes": 1048576,
  "maximum_trace_binding_uses": 262144,
  "maximum_metadata_rows": 4096,
  "maximum_probe_variable_bytes": 1048576,
  "maximum_trace_bytes": 2097152,
  "maximum_result_snapshot_bytes": 67108864,
  "maximum_report_bytes": 8388608
}
```

Authored probes use two current instances sharing local index 42. Literal bit
expectations and complete snapshot comparisons establish this opt-in host copy.
They do not establish original ObScript conversion, foreign assignment parity,
retail scheduling, playable routes or gameplay acceptance. The executable used
for CLI descriptor inspection is an immutable local metadata copy and is never
launched.
