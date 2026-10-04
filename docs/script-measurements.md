# Source-bound semantic trace comparison

`fallout script-trace` imports original and replacement observations and compares
them against a manifest and the existing prepared winning script plan. It does
not launch either engine. The isolated original profile, launch and recorder are
owned by the coordinator; native observation glue must be disclosed separately.

```powershell
fallout script-trace --install <isolated-source-copy> --load-order <order.json> `
  --manifest <case.json> --profile-receipt <profile-receipt.json> `
  --original-trace <original.json> --replacement-trace <replacement.json>
```

The runner checks the profile receipt's actual SHA-256 and executable file bytes,
loads the existing catalogue and prepared sources, and binds the manifest to the
exact definition handle, complete source receipt cohort, winning content digest,
compiled digest and byte length. Each
step names its source event ID/begin offset, operation offset, event ordinal,
explicit normalized caller roles, operand bits and optional item FormKey.
Assignment/conversion probes must name an assignment envelope, branch probes a
conditional envelope, and GetItemCount probes the existing command identifier
at an instruction or expression-token position. Its item must match the encoded
source reference-table argument. Observed successors must name instruction
headers. These checks establish source
relationships, not operation semantics.

The schema is `execution::trace::{Manifest,Capture}` version 1. A capture records
its producer, actual producer executable digest, transport receipt digest,
instrumentation disclosure, completion state, ordered inputs and observed output.
Outputs retain a numeric return, successor offset, ordered local writes and an
observed error where present. Numeric words carry an explicit binary64, binary32,
signed32, unsigned32 or unsigned64 format and fixed-width lowercase hexadecimal
bits. NaN payloads and signed zeros are compared exactly. There is no conversion
through decimal JSON or a tolerance inferred from matching samples.

Missing captures, interrupted/unsupported execution and absent required outputs
return `blocked`. Different identity, producer, inputs, ordering or outputs return
`mismatched`, with a deterministic first difference. A matching import returns
`matched`; the CLI exits nonzero for blocked/mismatched reports. Every report
keeps `gameplay_accepted`, `faithful_execution_admitted` and
`retail_execution_performed` false. External recorder authenticity is outside
this importer: a supplied digest is an identity receipt, not authentication.

JSON inputs are bounded to 4 MiB per manifest/capture, the profile receipt to
1 MiB, and the executable metadata input to 64 MiB. Production comparison bounds
steps, operand words and local writes separately. Strict serde fields reject
unknown or duplicate struct fields. These are admission bounds, not a total heap
or decompression CPU measurement.

The focused regressions exercise four probe purposes with authored compiled
fixtures, deliberately alter return bits, branch destinations, writes, caller,
inputs, event order and identity, and check missing/incomplete traces. Existing
local-copy staging/commit produces an actual replacement observation in one
test. Existing native dispatch produces an engineering inventory count in
another; its unavailable original numeric return remains blocked. Synthetic
original-labelled test data is explicitly disclosed and is never retail proof.

The first original assignment/conversion/branch/GetItemCount captures are still
required. The importer does not implement unmeasured numeric coercion, branch
selection, original scheduling or a native return. Those remain unsupported in
the faithful runtime. No source text is required by this consumer.
