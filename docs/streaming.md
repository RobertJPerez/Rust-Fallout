# Cell residency source boundary

`world::residency::CellResidency` owns one requested cell over the existing sealed
`CellModelPlan`, `ResourceJobs`, archive importer and schema-1 cache. Presentation
and physics consume the same immutable model bytes. It owns no persistent
references, GPU resources or collision geometry.

Create the owner once with a private cache and explicit limits. `request(plan)`
returns an unforgeable `Ticket` with the exact CELL, source-plan digest and owner
generation. `poll()` performs at most eight completion checks and eight admissions;
IO and BSA decoding run on existing bounded workers. Source planning, archive
indexing and fingerprinting still occur before the request and need a host worker.

Stages are `Unrequested`, `IoPending`, `Decoded`, `DependenciesReady`,
`RenderResident`, `Unloading` and `Failed`. `Decoded` means selected BSA payloads
are decoded and retained, with no assertion about NIF block semantics. After that,
`sources(&ticket)` yields an `Arc<ResidentSources>` whose `plan()` and `model(i)`
borrow exact sealed metadata and bytes. Use the existing NIF scene/material,
animation and collision decoders; consumers bound their own resulting arrays.
`plan()` returns a borrowed `ResidentPlan` view with `graph()`, `receipt()`,
`root()` and `identity()`. It exposes no cloneable `CellModelPlan` or archive
input; retain the source Arc while using its borrowed metadata and bytes.
Actor-specific model roles still require their existing dependency producers.
Missing or ambiguous MODL sources remain visible in plan coverage.

The source worker calls `TexturePlan::load(sources.clone(), mounts, limits)` using
the same existing `MountIndex`. It derives external texture paths through the
existing bounded NIF scene/material decoder, retaining model hashes, block/slot,
authored byte paths, path errors, all lookup candidates and unsupported blocks or
scene edges. It deduplicates only unique safe normalized paths. Ambiguous sources
remain unresolved; this does not invent archive/loose precedence or repair absolute
exporter paths. Selected archives are protected and fingerprinted by the existing
importer before their requests are sealed. Source planning can block and belongs
on the source worker, outside rendering/polling callbacks.

`request_textures(&ticket, plan)` attaches one texture batch per cell generation.
Its exact model lease must belong to this owner. `poll()` admits/extracts textures
using the same resource jobs, byte pool, cache and cancellation gate as models.
`texture_sources(&ticket)` returns an `Arc<ResidentTextures>`; `texture(i)` borrows
bytes ordered by the sealed receipt's requests. Retain this Arc through DDS decode
and GPU upload; those operations remain presentation's responsibility. No archive
input, cache artifact or resource-bearing model plan can escape its source pins.
Cloned texture plans retain their model lease and texture mappings/metadata too.

`report_dependencies(Ready)` requires an admitted texture plan and all its captured
external paths uniquely resolved and decoded. Even a model without external
textures needs the explicit empty plan. Partial decoded texture bytes can be
inspected while missing inputs keep readiness blocked. Snapshot texture state,
requested/completed counts, coverage and sealed identity expose this distinction.
Unknown NIF blocks/scene edges keep reference coverage unverified and prevent
full `simulation_ready`. A consumer may explicitly report and render its supported
subset once captured textures resolve; that subset does not prove full material,
shader, embedded texture or pixel semantics.

Each consumer decodes or stages resources outside the generation lock. Report
dependency, collision and behavior readiness using the same ticket. Missing or
unsupported collision prevents `simulation_ready`, even when render admission
succeeds. Simulation also requires explicit dependency/behavior readiness and
complete model coverage with resolved/null source graph links. These are host
engineering reports; reporting Ready is no evidence of retail units, activation,
navigation or script semantics. A faithful consumer must refuse unverified inputs.

`publish_render(&ticket, closure)` gates final host/entity admission against
replacement, cancellation and owner drop. Keep its callback short, nonblocking
and free of calls back into ticket/owner methods. GPU staging happens beforehand.
Failed admission leaves world residency unchanged; the host cleans up its own
partial resources. Callback effects are not an automatic transaction or rollback.
Each request generation permits one successful render publication. Repeated
admission refuses before calling the callback, including after dependency
downgrade/recovery. `Snapshot.render_published` retains that fact. A failed
callback can be retried; `retry()` issues a fresh epoch that permits publication.

`unload()` invalidates all tickets, cancels owned requests and drops owner-held
source pins. Presentation and physics release their own resources. Runtime keeps
persistent existence. A borrowed old source lease remains charged to the same pool
until its final Arc drops, and cannot admit another render or collision result.
`Unloading` becomes `Unrequested` after all old worker/output reservations drain.
`retry()` issues a new generation and reuses only verified cache artifacts.

Enforced ceilings are two workers, 1,024 combined outstanding/retained model and
texture payloads and 256 MiB
of queued/running/retained decoded payload. Whole-cell source bytes and model
count are checked before revoking a working request. The pool admits the whole
bounded batch: retaining outputs in an eight-slot pool would stall larger cells.
Every poll still has a fixed work ceiling. Old borrowed payloads can backpressure
a replacement; dropping them releases capacity. Keep this owner across requests
to retain its accounting. Do not create a new owner to escape held source quotas.

Plan pins separately bound two admitted/retired plans, 32 MiB of conservative
plan metadata and 16 GiB of mapped source extents. They remain attached to queued,
running and retained artifacts after cancellation. Even a source batch with zero
selected models keeps its plan charged while a consumer retains that lease.
Replacement admission reserves the new plan before revoking the old one; an
over-budget request preserves current residency. Source extents count conservatively
per plan rather than claiming exact resident pages or cross-plan map deduplication.

Texture plans share those metadata/mapped-extent counters. They charge before
retaining metadata or mapping an archive, with local ceilings of 8,192 external
references, 1,024 unique texture requests, eight archives and 8 MiB of conservative
metadata. Model plus texture count/decoded bytes must fit the owner's combined
`Limits.resources`/`source_bytes`; `models` still separately bounds model requests.
The existing NIF decoder has separate limits of 100,000 blocks and 128 MiB of
temporary arrays per model. Receipt sealing streams bounded JSON without a second
metadata allocation. Importer index allocations remain under its existing limits;
these counters do not measure the process heap. Stale planning cannot charge or
resubmit; old plan/source clones remain charged until their final release.

These limits cover source payload/work, not exact process heap or GPU memory.
Sealed plans retain the existing bounded graph/metadata/archive map allocations;
archive indexing and source mapping retain the existing importer limits. Consumers
must budget additional decoded arrays and release obsolete leases. Cancellation
does not interrupt the pinned decoder inside one bounded extraction. Owner drop
joins only its own workers at that boundary; unload/poll remain nonblocking.

Controlled authored tests cover more than eight sources, decoder consumption,
readiness gates, stale/foreign admission, unload with queued/running work, retained
pin backpressure, cache reuse, corruption, failed publication and owner drop.
Worker ceiling and repeated-publication regressions cover downgrade/recovery and
fresh retry; a compile-fail source accessor test rejects detached plan cloning.
Texture tests additionally cover batches above eight, unique-path deduplication,
missing/ambiguous/absolute paths, unsupported NIF metadata, shared exact/one-over
quotas, foreign/repeated admission and controlled running/queued cancellation.
Application wiring belongs to presentation; collision readiness belongs to physics.
This boundary alone does not establish playable traversal or retail parity.
