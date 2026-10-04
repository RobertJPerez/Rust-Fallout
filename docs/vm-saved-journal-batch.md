# Explicit saved engineering journal prefix

`execution::pending_batch::consume` moves an already strictly restored private
World into a bounded operation. Its explicit ordered Request list names each
existing pending sequence and Fragment activation. It checks the complete prefix
and its owners before any private commit, then reuses the unchanged
`copy_probe::commit_pending` single-copy engineering host. It creates no instance,
reference, local initialization or event. Each existing event receives its normal
canonical one-assignment/one-acknowledgment revision. It returns the final snapshot
and ordered receipts only when the whole prefix succeeds. A later refusal/error
discards the owned private World and earlier traces; no partial successful result
is exposed. Faithful intent returns a typed unverified-semantics refusal.

This concerns several existing journal events. VM22 concerns several assignments
inside one event and remains a separate opt-in operation. The single-copy event
shape and refusal rules remain Begin / one own numeric bit copy / End. Raw numeric
storage, including NaN payloads and signed zero, is preserved; this establishes no
original conversion, dispatch order or event-list lifecycle semantics.

Defaults bound 64 events, 192 aggregate prepared instructions and 65,539 aggregate
body statement bytes. Conservative trace admission reuses VM20's existing owned
EventObservation projection with cumulative 1 MiB source window bytes, 65,536 rows,
1 MiB UTF-8 variable bytes and 262,144 full-definition binding uses. It includes
both contexts and canonical values of own bound reads/destinations before calling
the old host. Temporary index rows are bounded before allocation, and the temporary
projection is dropped before retaining the old copy trace. The counts describe a
logical conservative superset of accepted trace payload, not actual encoded trace
bytes or peak heap. Binding scans have a fixed number of passes per admitted table;
the bound is not a hard time slice. No second parser or admission helper is added.

The actual CLI adds `event-operands --snapshot-copy-batch-request REQUEST.json
--snapshot-input BEFORE.json --snapshot-output AFTER.json --output REPORT.json`.
Its strict schema-1 request requires explicit engineering intent, events and all
aggregate/result/report limits. Requests are at most 16 KiB; event count is 1..64.
Caller source/trace bounds can reduce the defaults but cannot exceed their stated
ceilings. Result encoding is explicitly bounded at 1..64 MiB; the complete pretty
report plus newline at 1..8 MiB. The CLI restores strict current schema 4 once for
processing, prepares only distinct exact prefix definitions while keeping the
complete source cohort identity, and processes all events through that World.

Result and optional report are fresh artifacts outside the installation. The
consumer admits complete result encoding, strict cold decode/restore equality
and the whole encoded report before opening an output. Both use atomic
`create_new`, preventing input replacement through existing paths, hard links,
symlinks or racing creations. The report and result must differ. Unsupported
events emit an explicit refusal report with result null and a discarded private
result; they never create a result snapshot. Input/source/capacity errors create
neither output. An output I/O failure can leave an incomplete fresh artifact and
does not certify successful publication. Standard one-event/native/quest/seed
paths retain their previous behavior and explicitly conflict with the new mode.

Focused checks compare literal entire expected snapshots for two/three events,
including untouched trailing events, counters, unrelated script instance, enabled
state, signed-zero/subnormal pose, inventory19, NaN condition and opaque extras.
Fresh CLI processes prove cold next-head consumption and old-sequence replay
refusal. Later unsupported events, wrong prefix/activation, strict legacy/missing
instance/stale cohort, aggregate/result/report ceilings and one-under bounds,
input/report/result aliases, mode conflicts and immutable source/input bytes are
exercised. Authored fixture/metadata evidence is engineering-only; original
execution, faithful admission and gameplay acceptance remain unverified.
