# Explicit selected source plans

`PreparedSources::load_selected(catalogue, model, signatures, exact_handles,
Limits)` prepares only caller-selected exact script versions using the existing
definition preparer. It validates all handles before preparing any source and
rejects duplicate definition keys with `Error::DuplicateSelection`. Stale or
missing handles return `Error::Lookup(LookupError::DefinitionChanged)`. Source
preparation runs in canonical key order, independently of request order.

The private coverage vector borrows canonical catalogue sources, including
selected absent and rejected bodies. `get` validates exact existence/version
first, then returns `LookupError::NotSelected` for an uncovered definition.
This includes an unselected absent body: it is not a missing-script finding.
A selected absent body returns the existing `MissingBody` source finding;
selected rejected metadata or structure retains the existing cached rejection.
An empty explicit selection covers nothing. Full `load` and cooperative jobs
retain their existing full-cache lookup behavior.

The cache still binds the complete catalogue and decoder, with the same
catalogue/source/signature limits and world-cohort validation. Catalogue loading
and identity hashing are not skipped. `Counts.definitions` and `source_receipts`
describe that full input cohort; preparation attempts, absent fields, retained
plans/findings and attempted bytes count only selected work. Existing per-source
and aggregate preparation allowances apply to that work. Failure returns no
partial cache. This API neither discovers dependency closure nor grants
execution authority. An existing dependency consumer reaching an unselected
script receives `NotSelected` and must explicitly request a sufficient subset.

The existing CLI provides a source-bound consumer:

```text
fallout source-plans --install <read-only-source-copy> --load-order order.json \
  --selected-source request.json --output report.json
```

The strict request has `schema_version: 1`, the expected complete
`source_cohort_sha256`, and `definitions`, an array of exact loaded script
handles. The CLI admits at most 128 handles and 64 KiB of request bytes. Selected
rows preserve request order and report the source handle, compiled byte/hash
receipt, binding digest, structural counts or existing source finding. A source
finding returns nonzero; stale/duplicate handles, changed cohort and malformed
requests return no report. `execution_ready` and `retail_parity_accepted` remain
false, including for an empty selection.

This mode is separate from cooperative preparation, execution admission and
comparison-bundle writing. It creates no canonical state, event, effect, save
or original behavioral expectation. Its authored fixture contains a 26-byte
selected copy event, an unrelated 80,014-byte event with unavailable native
signatures, and an absent body. Full preparation attempts 80,040 source bytes;
the selected consumer attempts 26. The runtime checks additionally exercise
the existing dependency admission API, selected-plan/full-plan equivalence,
exact budget refusal and the complete world source guard. The ignored
`cli_selected_preparation_helper` uses a frozen CLI and an explicit executable
metadata copy; it never launches the original executable.
