# Source-bound engineering local copy

The coordinator-approved `VM-02-source-copy-v1` consumes runtime's tested
`STATE-02` staged transaction API. It adds no state bank, event journal,
continuation, identity allocator, dependency or save field.

`World::stage_source_local_copy_with_sources` prepares one exact pending block
against the immutable source cohort. Engineering intent admits only a Begin
header, one assignment and an End header. Its destination and sole expression
operand must name own declared numeric locals. Literal conversion, operators,
native calls, globals, foreign prefixes, reference slots and branching blocks
remain unsupported. Faithful intent returns `UnverifiedRetailSemantics` before
reading local storage: original assignment conversion and event lifecycle rules
have not been measured.

The adapter reuses the existing current-state operand probe, including its
canonical declaration/read validation. A successful engineering stage copies
the initialized source `Number` bits without evaluating them. It retains source
instruction/token extents, compiled statement bytes, owning definition, campaign,
cohort, revision, exact pending context, declarations, source value and prior
destination. Destination storage may be uninitialized. These observations do not
certify original numeric behavior, including cross-declaration conversion.

`StagedCopy` contains runtime's opaque nonserializable `StagedEventChanges`.
Dropping it has no effects. Consuming `commit` uses the canonical runtime API to
change exactly one local and acknowledge exactly the staged head, with one
revision increment. Intervening mutation, another world/restore, source mismatch
or a changed head rejects before effects. A trace is diagnostic evidence, never
authority for a later mutation.

Event instruction, operand-use and retained statement-byte limits apply before
the corresponding retained work. Source admission keeps the existing immutable
source budgets. No partial stage is returned on exhaustion or source failure.

## Explicit inspector consumer

`fallout event-operands --engineering-local-copy request.json` uses the existing
deterministic engineering harness. The bounded request is:

```json
{"sequence":1,"initial_numbers":[{"index":1,"bits":9223372036854775808}]}
```

All fields are required. Unknown fields, nonnumeric/overflowing bits, files over
64 KiB and more than 64 initializers fail. The exact first pending instance is
selected; canonical typed assignment validates every explicit initializer before
the source copy is staged. These are host fixture inputs, not inferred defaults.
The report distinguishes explicit initialization from the staged assignment.

The option implies prepared sources and produces schema 4. It conflicts with
`--native-capabilities`. The existing schema 1/2/3 modes keep their behavior.
Source operand observations precede explicit initialization; their revision and
snapshot hash continue to identify that read-only phase. The additional copy
report identifies the initialized and committed snapshots, verifies exactly the
one requested copy/head change, and checks same-process decode/restore with old
transient handles rejected. `canonical_state_unchanged` reflects the actual
opt-in harness changes. Unsupported source shapes emit a report then return 1;
they do not commit or acknowledge an event. No native save is published.

## Evidence boundary

Source layouts reuse the independently compared expression, operand and
definition readers. Authored source-less compiled cases specify fixed offsets
and numeric bit patterns independently of the adapter. They exercise signed
zero, NaN payloads, wide values, uninitialized destinations, exact budgets,
unsupported shapes and stale-stage rejection. A separate process restores the
committed authored snapshot and checks fixed values, campaign, revision and the
empty pending journal.

This is an engineering mutation path, not accepted original bytecode execution.
Original coercion, scheduling, event acknowledgment timing, general expressions,
native effects, continuation persistence and gameplay parity require separate
original measurements and coordinator-reserved retail capture.
