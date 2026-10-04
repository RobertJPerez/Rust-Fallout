# Cell model source preparation

`cell --include-dependencies --inspect-models` connects the existing cell
inspector to an exact source plan and bounded archive jobs. This opt-in path
adds `model_preparation`; the original cell and model inspection paths retain
their output shape. It is a source diagnostic, not a streamed or playable cell.

`world::preparation::CellModelPlan::load` constructs a sealed plan from a
protected `RecordStore`, the exact winning CELL key, and existing archive mount
candidates. It builds the bounded CELL/WRLD dependency report internally, then
reads each resolved root-member base with the existing MODL decoder. It records
the plugin cohort, full winning headers, decoded base hashes, physical MODL
header offsets, normalized lookup paths, and every archive candidate. Selection
uses only a single archive candidate. Missing MODL, missing archive sources and
ambiguous candidates remain explicit deferred coverage. Actor extras, alternate
models, loose lookup and retail precedence are not inferred.

Each selected member binds the protected archive digest, source length, entry
index, original path and source-declared decoded length. The plan identity hashes
the root, plugin cohort, base coverage and these actual archive receipts. Plugin
bytes are captured as owned source evidence; the RecordStore owns its plugin
handles. Archive handles remain pinned by plan clones, requests and Ready until
their last owner drops. Source evidence describes that captured cohort, not a
live filesystem watch. Source planning and existing archive indexing/hash reads
are synchronous; cancellation applies to the admitted resource-job phase.

`CellPreparation` reuses `resource_jobs::ResourceJobs`, its cancellation tokens,
strict cache checks and reservation lifetime. A poll consumes at most eight
completions and submits at most eight requests. Queue and decoded-byte limits
include queued/running work and unconsumed artifacts. Extraction and one existing
NIF container inspection are opaque within admitted byte limits; checks around
them reject revoked results. The existing NIF importer produces the probes.

Every requested model must succeed before `take_ready` returns a sealed `Ready`.
Probes remain private until `Ready::publish_into` validates the destination CELL
and base/MODL/archive bindings and passes the current token gate. Failures leave
the destination unchanged. Already committed source-addressed cache entries
remain valid after a batch fails; they never constitute partial cell readiness.
A self-consistent shortened cache marker/blob still fails against the pinned
member's decoded length.

Replacement and retry always advance the owner epoch, including an identical
plan. Queued, running, completed and already taken Ready objects from an earlier
epoch cannot publish. Cancellation drops queued handles and staged probes. The
preparation owner must remain alive until publication: dropping it closes the
resource pool and revokes outstanding Ready objects. A receipt records the
captured plan and publication epoch as historical diagnostic evidence.

The plan admits at most 1,024 bases/requests, 8,192 candidates and eight selected
archives, with lower-only limits. It checks remaining plugin-body, field-site,
path, model-byte and metadata allowances before their corresponding allocations
or reads. Default aggregate plugin bodies and selected decoded model bytes are
each bounded at 64 MiB. Metadata allowance is 16 MiB, in addition to a logical
probe reservation of `32 * decoded_member_bytes + 4096` per request, capped at
512 MiB. This conservative reservation covers admitted diagnostic work; it is
not a measurement or bound on total process heap, archive maps, existing store
indices, allocator overhead, JSON serialization or other lanes. Existing archive
and NIF reader ceilings also apply.

`all_requested_model_sources_ready` means the selected archive members passed
the existing NIF container inspection. Deferred model selection, unhandled NIF
block semantics, actor state, animation, scripts, collision, units, physics,
input and retail behavior remain unresolved. `runtime_ready` stays false in the
cell, dependency report, plan and preparation receipt. Presentation does not
become authoritative state.

Authored tests use actual plugin indexing and compressed BSA members to exercise
cold/warm reuse, changed plugin cohort with unchanged archive bytes, stale Ready,
cancellation, owner drop, aggregate failure, partial cache rejection, retry,
physical source binding, queue pressure, deferred coverage, budget failures and
archive handle release. Installed source inspection adds coverage evidence only;
it does not establish gameplay parity.
