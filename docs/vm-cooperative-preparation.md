# Cooperative immutable source preparation

`fallout_runtime::programs::PreparationJob` provides a host yield/cancellation
boundary while building the existing immutable `PreparedSources`. It borrows
the catalogue, structural model and native signature table. It does not decode
new formats, establish original scheduling behavior, or grant execution access.

`new(catalogue, model, signatures, Limits)` returns `Result<PreparationJob>`.
It performs the existing definition/source/signature admission and bounded
source-cohort/decoder hashing up front. `advance(StepBudget)` processes at most
the specified number of definitions and compiled source bytes. Absent bodies,
zero-byte bodies and rejected metadata all consume a definition allowance.
Every admitted definition is indivisible and retains the existing individual
and aggregate preparation caps. A body larger than the remaining step byte
allowance yields without decoding it. This is not a hard time slice; loading
the input catalogue and hashing its identity also occur before these boundaries.

Progress contains status, cumulative counters, counters for the current call,
and the next body's byte length when pending. It exposes no plans. Calls after
completion or failure do no further preparation. Successful plans and source
findings remain private and are never reparsed after a yield. `finish` consumes
the job and returns a cache only when every definition has been visited. Failure
returns the original aggregate admission error; incomplete finish returns
`Error::Incomplete`. Dropping a pending job discards its private plans without
changing the catalogue or a canonical World. The synchronous `load` method
drives this same builder to completion, preserving existing cache behavior.

The existing CLI consumes the API:

```text
fallout source-plans --install <read-only-source-copy> --load-order order.json \
  --cooperative-preparation request.json --output report.json
```

The strict request is at most 16 KiB:

```json
{
  "schema_version": 1,
  "maximum_definitions_per_advance": 4,
  "maximum_source_bytes_per_advance": 1048576,
  "maximum_advances": 4096,
  "cancel_after_advances": null
}
```

The CLI allows 1–4096 advances and positive per-call allowances. Cancellation
may be requested after zero or more calls, up to that total. It drops the job
at the requested boundary. A step that cannot admit its next indivisible body
also terminates by dropping the job; its progress reports the required byte
length. Exhausting the advance allowance does the same. These outcomes return
nonzero, retain diagnostic counters, and publish no cache receipt. A completed
cache reports its counts, complete source-cohort identity and decoder identity.
Source findings still return nonzero. All reports keep `execution_ready` and
`retail_parity_accepted` false.

This mode is separate from execution admission and comparison-bundle writing.
It creates no save, event, native effect, thread, or continuation. The runtime
checks compare completed plans and findings against the existing per-definition
preparer and exercise exact aggregate failures. The ignored
`cli_cooperative_preparation_helper` consumes a built CLI and an explicit
executable metadata copy to inspect 37 authored definitions in one, 13 and 37
calls, cancel at zero/two calls, refuse insufficient allowances, and complete
again after cancellation. It never launches the original executable.
