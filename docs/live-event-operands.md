# Operand observations for a pending event

`World::probe_event_operands` prepares one exact pending event, then associates
each selected source operand with explicit live storage. It keeps occurrence
order and duplicates. The result is an owned diagnostic observation carrying the
campaign, state revision, pending entry, definition version, event window and
full definition binding digest.

The method borrows the world. It does not assign values, consume an event, invoke
a native handler or execute bytecode. An observation grants no permission to
write later: an execution step must resolve storage again against current state.

The content and script catalogues must have the same complete source cohort.
This check applies even when the selected event uses only its own locals. Full
strict source preparation precedes event selection; an unsupported source plan
cannot produce a partial operand probe. The caller can cap selected uses, and
the underlying preparation retains its source and instruction limits.

## What the observations mean

| Source association | Observation |
| --- | --- |
| Local destination | Exact supported declaration; no initial value read |
| Local read | Exact stored numeric bits or reference payload |
| SCRV reference | Supported caller declaration and its stored reference value |
| SCRO reference | Explicit null, authored content identity, or explicit runtime binding |
| Foreign local | Actual registered quest/placed live event list and its declaration |
| Global operand | `unverified_global_value`; no value inferred from authored fields |

A first assignment can target an uninitialized supported local. Reading that
local still fails. Index zero and unknown storage types remain unsupported.
Direct native form-variable operands require a reference payload; they are
distinct from a reference-table index. Numeric bits, including NaNs and signed
zero, are retained without conversion or arithmetic.

An authored SCRI attachment cannot supply a missing foreign live event list.
The foreign target uses the definition actually attached to the registered live
owner, even when it differs from the authored attachment. A placed authored
reference needs an explicit live identity. A player dependency requires a
caller-supplied registered player identity. Null, content and live references
remain separate values.

A resolved source reference does not establish that a native command accepts
its form kind, caller context or value. Native argument readiness, coercion,
arithmetic, branch effects and original event lifecycle remain unverified.

## Inspector and independent audit

```powershell
.\target\release\fallout.exe event-operands `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles\nv-inspection-order.json `
  --output local\event-operands.json
```

The inspector restores the existing explicit engineering fixture and probes
every pending event. It records source failures and unresolved operands, writes
the complete report, then returns 1 when either remains. It checks the final
canonical snapshot against the initial snapshot. Report storage, repeated
source work and selected operand counts have separate limits; failed preparation
attempts also consume the repeated-source budget.

The native operand source reader has an additive `--export-uses` mode. Schema 2
exports exact 33-byte binding tuples from its independent compiled-source
reader. Its default schema 1 projection is unchanged. Exporting tuples grants
no execution permission and supplies no live values.

`tools/event_operand_comparison.py` joins every probe to separately persisted
journal entries and winning source metadata. It compares the complete selected
tuple sequence against the native export, including offsets, context indices,
storage declarations and optional-field presence. It checks exact saved storage
payloads and derives the counters again. Its fixture contract explicitly admits
fragment instances with unmapped live references and no player binding; it
rejects other fixture shapes instead of pretending to validate their lifecycle.
The optional `--negative-checks` mode rejects changed saved bits, unresolved
codes, omitted uses, shifted windows, changed binder/source identities, damaged
native tuples and hidden source findings.

Thirteen runtime tests cover supported and unsupported storage, destinations,
reference payloads, actual foreign owners, player bindings, restoration, exact
budgets, whole-cohort mismatch, source rejection and event-window isolation.
The SCRV storage check is shared by direct references and foreign contexts;
both report unsupported storage before trying to read an unset index-zero value.
The prepared-source variant also observes changed player bindings and replacement
foreign live lists. See [source preparation reuse](prepared-runtime-sources.md).

Private development capture checked 2,160 pending entries, 2,111 probes and
54,498 selected uses. The native tuples and saved storage observations agree;
49 source failures and 9,382 unresolved operand results remain explicit. Cold
and warm reports agree after excluding cache metadata. These are development
results until a fresh integrated source/binary proof publishes a receipt.

No retail VM state, gameplay scenario, native effect, execution continuation or
save-format extension is accepted by this slice.
