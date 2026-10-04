# Sequential engineering own-local copies

`World::stage_source_multi_copy_with_sources` is a separate opt-in adapter for a
complete prepared event containing one or more own numeric assignments. The old
single-copy adapter still requires Begin / one assignment / End. Both explicitly
refuse Faithful intent; original assignment conversion, initialization, branch,
event scheduling and event-list lifecycle are unmeasured.

The adapter reuses prepared statement/expression plans and the exact existing
source binding pairs. Each assignment must have one own Float/Integer read and
an own Float/Integer destination. Its private bounded overlay evaluates physical
source order. A read sees a preceding overlay write, including a write that
initialized previously unset storage. Otherwise the source must already contain
a Number. The adapter preserves the raw 64-bit storage pattern, including NaNs
and signed zero; it performs no conversion or arithmetic. Repeated destinations
collapse into a sorted, unique final assignment set.

No canonical state changes during preparation. Any later literal, operator,
native call, foreign/global operand, conditional or unavailable slot refuses the
whole event. Capacity or source failures also return no staged result. Only
runtime's existing opaque `StagedEventChanges` authorizes the final commit,
which applies all unique destinations and acknowledges the pending journal head
once, with one revision. Existing campaign, epoch, source, revision and exact
pending-head guards remain authoritative. Traces have no deserialization or
authority-import path and are not saved continuations.

`MultiTrace` retains the ordered reads/writes, original canonical source values,
whether each read used the overlay, effective destination-before values, exact
statement/token SCDA ranges and indices into one owned `EventObservation`.
VM20's bounded projection copies the source window, selected operand uses and
context once per event. Statement records do not duplicate that window. Per-event
defaults are 64 statements, 128 uses and 65,539 cumulative statement bytes;
preparation also caps admitted instructions. The projection defaults cap source
bytes at 1 MiB, retained rows at 65,536, UTF-8 variable bytes at 1 MiB and full
definition binding scans at 262,144 uses. These are logical work/retention bounds,
not a wall-clock or peak-allocator guarantee. Binding-index rows and cumulative
statement bytes are charged before their corresponding payload retention.

The actual standalone host accepts `script-trace --replacement-multi-copy
REQUEST.json` mutually exclusively with imported replacement or single-copy
mode. It uses the existing strict Request schema 1 and finite explicit numeric
initializer policy. The existing initializer limit also bounds seeded declared
local storage. Manifest observations sharing an event ordinal must name the same
complete event, cover every assignment exactly once in physical order and match
actual overlay input words. Each group enqueues and commits one event; multiple
statements do not invent multiple event lifetimes. Across groups, `MultiProbeLimits`
charges cumulative statement bytes and projection bytes/rows/binding work before
retaining another observation. A discarded private preview validates the entire
requested sequence before the result world is run. A later refusal exposes the
unchanged initial snapshot and no successful partial capture.

The new mode emits report schema 2 with its request hash; existing modes keep
schema 1. The complete encoded new report, including its newline, is limited to
8 MiB before emission. Input JSON remains bounded at 4 MiB; provenance binds the
exact executable, profile receipt, complete runtime cohort, winning content,
compiled bytes and source sites. Missing original capture remains Blocked and
the CLI exits unsuccessfully even when the replacement capture completes.
Authored probe success is engineering evidence, not faithful execution admission,
whole-route readiness, gameplay acceptance or an original observation.
