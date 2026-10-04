# Existing saved pending engineering copy

`execution::copy_probe::commit_pending` consumes an already source-restored
`World`, exact pending sequence, explicit fragment activation and
`local_copy::Intent`. It calls the existing source local-copy stage and opaque
commit APIs. It creates no references, instances, local banks or journal entries.
An activation must match the saved instance's `Owner::Fragment`; a sequence must
name the existing journal head. Quest and placed owners are outside this host
operation's scope.

The existing adapter admits only BEGIN / one own-local numeric bit copy / END.
It reads the existing initialized source slot and writes the existing declared
destination slot. A literal, conversion, operator, native, foreign local or
branch stays unsupported. Faithful intent stays unsupported before live reads.
Canonical commit supplies revision/epoch/campaign/source guards, one assignment
and one head acknowledgment. No original scheduling or conversion claim follows
from an engineering bit copy.

The separate actual CLI consumer is:

```powershell
fallout event-operands --install PRIVATE_SOURCE_COPY --load-order order.json `
  --snapshot-copy-request copy.json --snapshot-input before.snapshot.json `
  --snapshot-output after.snapshot.json --output copy-report.json
```

Its request has all four fields explicitly present:

```json
{"schema_version":1,"sequence":1,"activation":1,"intent":"engineering"}
```

Requests are at most 16 KiB, reject unknown fields, and require nonzero sequence
and activation. Faithful/unspecified intent and initializer fields refuse. The
snapshot is bounded by the runtime snapshot byte limit and decoded using strict
current schema 4 admission, then restored against the exact complete source
cohort. There is no legacy migration fallback or engineering seed world. The
consumer prepares only the exact existing head instance's definition with
`PreparedSources::load_selected`; complete source/decoder identities remain in
the report. Existing source/statement/operand limits still apply.

The output's parent must exist outside the installation. An existing output
refuses. After successful canonical commit, the consumer encodes and verifies
the resulting snapshot through current decode/restore, then opens the requested
artifact with `create_new`, writes it and synchronizes the file. Unsupported
operations produce a report with `result_snapshot: null`, unchanged state and
no acknowledgment; they create no result snapshot and exit unsuccessfully.
Input/admission errors also create no result snapshot. An I/O failure during
output writing can leave an incomplete fresh file and never reports successful
publication; this raw snapshot artifact is separate from native save rotation.

The report retains the existing source copy trace and canonical receipt, input
and result snapshot hashes, explicit activation/intent, campaign, revision and
selected preparation accounting. `faithful_execution_admitted` and retail
parity remain false. This host operation is separate from the standalone seeded
microfixture observer and leaves that observer's behavior unchanged.

Focused checks compare the complete expected snapshot after cold restore,
including an unrelated authored reference's signed-zero/subnormal pose, explicit
enable state, inventory item facts/raw bits, counters, other locals and the next
journal entry. The actual fresh CLI commits a saved head and rejects cold replay
of its old sequence. Independent rejection cases cover activation/head changes,
strict legacy/cohort refusal, unset input, unsupported literal, explicit intent,
request budgets and protected/existing output. The helper uses authored source
and a read-only metadata copy of the original executable; it never launches the
original game and supplies no original behavior expectation.
