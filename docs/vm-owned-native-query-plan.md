# Owned source-native engineering query plan

`execution::native_plan::prepare(world, sources, content, selection, limits)`
creates an owned `Plan` only from current `NativeCalls` admission for an exact
GetItemCount occurrence at the existing journal head. Selection explicitly names
sequence, occurrence, subject/player inputs and intent. Faithful intent retains
the existing typed refusal. The old one-shot native API and outputs are unchanged.

The plan has private fields, no public constructor, serialization,
deserialization or commit method. It reuses `query::Request`, which owns canonical
campaign, full source cohort, subject and item identities. No World borrow, epoch
handle, cached count or original numeric return is retained. Read-only getters
expose historical diagnostic proof; that proof cannot be supplied as authority.

`Plan::observe(world, sources, content, maximum_contributions)` requires the same
campaign, complete source cohort, decoder, exact definition version and binding,
selected event, journal head, instance owner and both original contexts. It checks
the current physical call/prefix/argument spans and bytes, then uses the shared
native admission again. A changed resolved caller or item refuses. A removed
explicit caller, unavailable argument or inventory bank retains typed unsupported
behavior. Only current inventory is queried. Revision and epoch changes caused
by an inventory update or strict cold restore are allowed; the journal is never
acknowledged and no state changes.

Creation bounds cover the whole event instruction/call/argument inventory,
existing event-observation source bytes/rows/variable strings/full binding scan,
and retained query/item/decoder strings before cloning. Default plan ceilings are
4096 event instructions, 128 calls, 65539 argument bytes, the existing observation
limits, and 1024 query variable bytes. Query contributions have a separate cap.

## Strict saved consumer

```powershell
& .\target\debug\fallout.exe event-operands `
  --install .\local\authored-source-copy --load-order .\local\order.json `
  --snapshot-native-plan-request .\local\native-plan-request.json `
  --snapshot-input .\local\initial.snapshot.json `
  --snapshot-native-current .\local\current.snapshot.json `
  --output .\local\native-plan-report.json
```

The request is strict schema 1, at most 16 KiB, with all fields required:

```json
{
  "schema_version": 1,
  "sequence": 1,
  "occurrence": 0,
  "intent": "engineering_observation",
  "supplied_subject": 1,
  "explicit_player": null,
  "maximum_source_instructions": 4096,
  "maximum_calls": 128,
  "maximum_argument_bytes": 65539,
  "maximum_trace_source_bytes": 1048576,
  "maximum_trace_rows": 65536,
  "maximum_trace_variable_bytes": 1048576,
  "maximum_trace_binding_uses": 262144,
  "maximum_query_variable_bytes": 1024,
  "maximum_contributions": 65536,
  "maximum_report_bytes": 8388608
}
```

Callers may reduce these ceilings. Both snapshots are strictly decoded and
restored under current schema 4 and the supplied complete source catalogue;
neither input is modified. The initial World is explicitly dropped before the
current file is read and restored. The plan is evaluated against the current
World, and the entire current canonical snapshot is compared afterward. Inputs
may name the same file. An initially absent inventory bank does not prevent plan
creation; the current bank determines query availability and result.

The complete pretty report and trailing newline must fit the caller's cap
(1 byte through 8 MiB) before any report publication. A supplied report path must
be fresh and outside the installation; shared `emit` uses `create_new`. There is
no result snapshot. Semantic refusal produces a bounded unsupported report and
nonzero exit. Identity, strict restore, budget or source failures expose no
partial report. An I/O failure during report publication can leave an incomplete
fresh artifact, which is not a completed proof.

This is opt-in engineering observation over authored canonical state. It admits
no new native handler, branch execution, activation, scheduling, native numeric
coercion, original behavior or gameplay parity. Original captures remain required
for faithful ObScript effects and conditions.
