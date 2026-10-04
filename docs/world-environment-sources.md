# Explicit CELL environment links

`world::environment::CellEnvironmentSources::load(store, cell, limits)` prepares
one immutable source request from a strict winning NV CELL. Borrowed `receipt()`,
`root()` and `identity()` expose its retained evidence; report JSON cannot
construct authority. `validate_sources()` checks the complete ordered plugin
cohort, including exact names, count, file sizes and SHA-256, before reuse.

The complete CELL declaration in pinned xEdit FNV commit
`9fb016884bec138ea6c7b872cec831537d464c3e` defines these exact target domains:

| CELL declaration | Expected target header kind |
| --- | --- |
| XCCM climate | CLMT |
| XCIM image space | IMGS |
| XEZN encounter zone | ECZN |
| XCAS acoustic space | ASPC |
| XCMO music type | MUSC |

The factory reuses the existing CELL decoder, subrecord visitor, protected
bounded Store, source-cohort seal and master/winner resolver. Each present link
requires four bytes. It retains the raw u32, canonical target key, expected-kind
status and actual winning target header/plugin/source ordinal/SHA-256. Null,
missing, deleted and wrong-kind targets remain distinct; tombstone and wrong-kind
winners retain header evidence. An absent field produces no row. No WRLD input,
older winner, inherited setting or default supplies a missing declaration.

Rows remain in authored source order. Every row includes its zero-based decoded
subrecord header ordinal, visitor callback ordinal, complete field framing and
decoded header/payload/frame offsets. The header ordinal counts a validated XXXX
prefix; the callback ordinal excludes it. The existing visitor only permits one
ten-byte prefix, and its validated offsets determine these counts without an
additional parser. Uncompressed fields retain their physical frame offset;
compressed fields retain stored-body bounds and decoded spans with no invented
physical frame offset.

`header_source_available` means that the winning target header has the expected
kind and is live. Target bodies are deferred and their parameters and behavior
are unknown. This request does not decode climate/weather, image-space, music,
acoustic or encounter behavior, choose any active environment or admit rendering,
audio, respawn or gameplay work. `behavior_status` states that boundary for every
row. The strict CELL body hash and whole source fingerprints bind these header
inputs to their original source cohort.

| Lowerable construction allowance | Fixed ceiling |
| --- | ---: |
| Ordered source plugins | 256 |
| Retained CELL/target identities | 6 |
| Actual decoded subrecord headers including XXXX | 65,536 |
| Declared environment link rows | 5 |
| Strict stored/decoded CELL body | 4 MiB |
| Aggregate max(stored, decoded) requested-body reads | 4 MiB |
| Retained raw field frames | 256 bytes |
| Conservative retained/temporary metadata | 16 MiB |

Admission precedes source/key/header/frame copies and the bounded CELL read.
Temporary CELL full-name decoding is admitted before the existing decoder runs.
Metadata excludes the caller's existing index and allocator overhead; source
fingerprinting is separate from requested-body accounting. Unsupported widths,
duplicates, malformed/truncated framing, recovered tainted CELL bodies, source
changes and exceeded budgets produce no partial request. Request identity binds
the exact source declarations independently of the selected lower limits.
`runtime_ready` remains false.

`cell-environment-sources --install ... --load-order ... --cell Origin.esm:hex`
is the owned headless consumer. Optional private `--index-cache` reuses existing
protected metadata. The report includes the immutable source-ordered receipt,
plugin/order fingerprints and five explicit per-field declaration, resolution,
header availability and unknown-behavior statuses. It opens no archives or jobs.

Successful parsing emits a prepared source request and exit0 even when individual
declared headers are unavailable or fields are absent. That result means an exact
source closure, never active environment behavior. Factory refusal emits null
authority/source error then exit1. Earlier strict source-index framing failures
return the original error without constructing a report/request. All selection,
parent evaluation, target-body behavior and runtime flags remain false.
