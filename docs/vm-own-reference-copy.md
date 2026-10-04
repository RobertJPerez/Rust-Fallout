# Explicit engineering own reference identity copy

`execution::reference_copy::stage` admits one exact existing journal head with
Begin, one Assignment, End. Both operand indices must refer to own canonical
`schema::Kind::Reference` locals. This kind comes from the existing authored
declaration and SCRV association; the adapter does not invent a reference type
byte. It checks the actual first loaded declaration's type byte and decoded
offset against both physical source bindings, using the indexed
`LoadedScript::declaration` lookup. The existing f/s/l local token encoding is
required, without a context prefix. Globals, literals, calls, operators, foreign
reads/writes, numeric declarations and additional statements refuse.

The current source value comes from `probe_event_operands_with_sources` and must
be an initialized `Value::Reference`. `ReferenceValue::Live` retains the complete
u64 identity, `Content` retains the exact canonical FormKey, and an explicitly
stored `Null` remains Null. Uninitialized storage never supplies a default.
There is no integer/float conversion, player/default context, reference creation,
source form lookup or new resolver.

The private stage uses the existing `StagedEventChanges`. Its consuming commit
assigns one own slot, acknowledges one journal head and increments revision once.
An intervening mutation, another world, changed source or cold-restored epoch
rejects the stage before effects. Historical trace data is serialize-only and
cannot authorize a write. `Trace::copied_reference` reads the already retained
source projection; a redundant reference payload is not added to the trace.

Creation bounds cover event instructions, two operand uses, statement bytes and
the event observation's source bytes, rows, full binding table and UTF-8 strings.
The observation charges both selected local values before copying them; a copy
onto the same local captures that local once. Three times its variable-byte
count reserves the additional own-probe and canonical-stage payload copies,
including large Content keys. The complete compact diagnostic trace is admitted
before staging. These are logical payload bounds, not allocator accounting.

The separate saved consumer is:

```text
fallout event-operands --install COPY --load-order ORDER.json
  --snapshot-reference-copy-request REQUEST.json --snapshot-input INPUT.json
  --snapshot-output RESULT.json --output REPORT.json
```

For existing Fragment activation 1 and head sequence 1, a request can use these
ceilings. The owner and sequence must match the actual strict current input.

```json
{
  "schema_version": 1,
  "sequence": 1,
  "owner": { "kind": "fragment", "activation": 1 },
  "intent": "engineering",
  "maximum_source_instructions": 4096,
  "maximum_operand_uses": 2,
  "maximum_statement_bytes": 65539,
  "maximum_trace_source_bytes": 1048576,
  "maximum_trace_rows": 65536,
  "maximum_trace_variable_bytes": 1048576,
  "maximum_trace_binding_uses": 262144,
  "maximum_probe_variable_bytes": 3145728,
  "maximum_trace_bytes": 2097152,
  "maximum_result_snapshot_bytes": 67108864,
  "maximum_report_bytes": 8388608
}
```

All fields are required, unknown fields refuse, requests are at most 16 KiB,
and budgets may reduce but cannot exceed ceilings. Quest and Placed owners must
also match the actual head exactly. The consumer restores current schema only,
prepares the exact head's source definition, and commits inside a private World.
The complete result must encode/decode/cold-restore equal before publication.
Result bytes and the complete pretty report plus newline are bounded before
either output exists. Outputs are fresh, outside the protected installation and
distinct under Windows ASCII case folding. The shared report writer and common
saved-copy output helpers preserve the foreign-copy path's behavior. A strict
identity/source/capacity error creates no outputs. Typed unsupported behavior
creates a bounded refusal report, no result, and a failing CLI status.

Authored probes verify typed content/null/live identities, including values above
2^53, exact complete snapshots and untouched unrelated state. The opt-in operation
is host identity copying. Original ObScript reference assignment, conversion,
lifetime and scheduling remain unverified; Faithful intent refuses. Descriptor
inspection uses an immutable local executable copy without launching it. These
results do not establish retail parity, playable routes or gameplay acceptance.
