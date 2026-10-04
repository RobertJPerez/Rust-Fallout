# Archive resource preparation

`WORLD-01-resource-jobs-v1` adds fixed archive-member work to the existing
preparation/cache pipeline. The coordinator approved this contract on October 3,
2026. `model_probe::inspect_models`, reached by `fallout cell --inspect-models`,
submits each unique candidate to these workers and consumes its validated result.
The inspector keeps deterministic path order and its existing source report.

`ArchiveInput` uses `NvArchive`, retains its immutable mapping and Windows
write-denying source handle, and hashes that protected archive once. A `Member`
binds original entry index and path, decoded length, profile, source SHA-256 and
the existing `nv-bsa104-decode-v1` transform. It cannot change archive precedence:
ambiguous candidates remain unsupported. Other platforms still require callers
to keep mapped sources immutable, as documented by the existing reader.

`Generation` binds a SHA-256 request-owner identity and a monotonic generation.
The inspector's owner digest describes its selected cell/model requests; it is
not a whole-installation baseline or gameplay identity. Every member also carries
its actual archive source identity. Advancing the owner generation invalidates
old tokens even when the source digest is unchanged. A pool accepts only tokens
from its own controller. Obtain a fresh token for each independently cancellable
job; cloned tokens share cancellation.

`ResourceJobs` has explicit worker, outstanding-request and decoded-byte limits.
Defaults are two workers, eight outstanding requests and 256 MiB of admitted
decoded payload. Queue admission reserves the declared output before extraction.
Reservations cover queued/running requests and unconsumed completion, including
failure receipts. Successful outputs keep their allocation and source pin until
the consumer drops them. `Artifact::bytes` borrows the payload so moving the
payload out cannot silently detach it from its reservation. Consumers remain
responsible for any additional copies they allocate.

These are payload/work admission limits, not an exact process-heap or mapped-file
budget. Archive indexes retain the production reader's existing entry/name limits;
hashing has bounded scratch space, cache manifests have a 64 KiB limit, and worker
threads/backend decompression state add overhead. GPU uploads and downstream NIF
allocations retain their separate existing budgets.

Cancellation is cooperative before and after one bounded backend extraction,
between staged cache-write chunks, before publication and during result admission.
The pinned decoder has no callback inside decompression; an in-flight extraction
finishes before its obsolete output is discarded. Initial archive indexing and
fingerprinting happen while constructing an immutable input. Neither this API
nor the synchronous inspection consumer promises immediate IO/decompress abort.

Final cache renames and consumer admission share the generation/cancellation gate.
A token that becomes obsolete before its manifest commit cannot publish a marker.
A source-addressed artifact committed before invalidation remains valid cache
data, while its obsolete completion cannot enter the newer consumer generation.
Dropping the pool closes its controller, cancels queued work and joins only its
own bounded workers. Dropping a handle cancels its request and releases unconsumed
output after any already-running extraction reaches its cancellation boundary.

The existing schema-1 cache manifest remains the completion marker. Blobs and
markers stage in temporary files, sync, and publish without clobbering. Complete
orphan blobs are checked against freshly extracted source bytes before retry
publishes a marker. Corrupt markers/blobs are errors; names alone never admit a
partial result. Cold and warm paths both bind source/transform identity and
verify payload length/digest. No source files, persistent instances or save
formats are written.

Validation covers authored compressed/uncompressed BSA104 bytes, a real
model-inspector consumer with an authored NIF container, late/stale extraction
and completion, cancellation after marker staging, retry, queue/byte rejection,
wrong source/controller identity and release of source/output reservations.
The existing cache regression terminates actual writer processes at blob staging,
blob publication and marker staging. Commands and actual results belong in the
private handoff; these tests do not establish retail behavior or gameplay parity.

The preparation index plan remains a dry run. A persistent dependency graph/job
journal, full transitive conversion invalidation, streaming cell activation,
collision/navigation readiness, pose/GPU readiness and both gameplay input paths
remain later work. Presentation does not become authoritative state.
