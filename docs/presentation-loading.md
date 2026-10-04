# Responsive inspection source loading

The existing model, CELL and terrain inspector creates its window before starting
source preparation. `WindowCreated` for the primary window admits one preparation
worker; headless captures start the same worker on the first update. The window
title shows preparation, upload and failure status. Loading allows close input
while suppressing camera actions, and the input boundary quarantines held actions
when a completed scene enters its camera context.

Source preparation runs outside the window update. Progress replaces one string
of at most 200 characters; a single completed result waits for admission. The
worker rechecks cancellation between source stages and after bounded decoders
return. The window polls without waiting for extraction or joining an active
worker. A cancelled or superseded epoch cannot yield an admitted result. Closing
the window cancels this host's request; opaque source operations finish their
existing bounded work before dropping their result.

CELL preparation consumes the sealed `CellModelPlan` and existing `CellResidency`
jobs. It keeps the returned `Arc<ResidentSources>` and `Arc<ResidentTextures>`
through model adaptation, staging and render publication. A captured texture plan,
including an explicit empty plan, closes dependency readiness. The model adapter
uses existing scene/material and DDS decoders over those retained exact bytes;
missing captured textures cannot trigger another archive lookup. The borrowed plan
view supplies placement/request provenance without detaching a cloneable plan.
The residency owner is retained behind a mutex for Bevy's shared-resource type
requirement. Main-thread access uses exclusive `get_mut`, without waiting on a
background lock.

Draw preflight runs on the preparation worker. Admission submits at most eight
mesh/material/image assets, 16 MiB of payload and 128 entities per update. One
resource over 16 MiB is refused. Aggregate ceilings are 512 MiB of submitted draw
payload, 2,048 models, 4,096 images, 16,384 parts and 65,536 entities. Source
decoders and world jobs retain their own stricter limits. These are CPU submission
limits, not measurements of driver memory or frame duration.

Staged entities inherit a hidden root. Only a complete current scene reveals
that root. CELL visibility publication runs inside the current residency ticket's
once-per-epoch `publish_render` callback. Failure and cancellation close further
upload/publication and hide the root immediately. Disposal retires at most 128
owned entities and eight resources/16 MiB of declared payload per update. Reverse
creation order removes children before their parents, keeping relationship
cascades inside that entity bound. Submitted asset handles and unsubmitted
image/mesh/instance payloads remain owned until their retirement completes. A
returned original epoch cannot revive retirement. Unrelated entities/assets are
preserved. The host retains a disposing phase until cleanup finishes; an actual
app exit may release the remaining app resources outside the frame loop. Retry
remains subsequent VIEW09 work. These limits do not measure driver reclamation or
physical frame time.

Captures begin their 64 settling frames only after admission. The source-worker
timeout is separate from the subsequent GPU capture timeout. Failed source work
does not reach the ready/capture phase; its diagnostic remains in the window
title, or produces a failing headless exit.

The private CELL inspection report uses schema 3 to retain the pre-upload source
residency snapshot. Its render counts describe the prepared static subset;
omitted references and model errors remain recorded. Dependency readiness covers
that subset. Collision and behavior are explicitly unsupported, simulation is
false, and the snapshot does not assert GPU publication or gameplay readiness.

Headless host tests exercise gated workers, actual window-created and close
messages, capture admission, visible failure, Bevy asset batching and stale
cleanup. Material and terrain captures separately check that the new loading
path preserves their existing rendering. Commands, results and evidence hashes
belong to the corresponding tested handoff. Physical window responsiveness and
keyboard/controller feel still need a working desktop automation connection or
manual validation; engineering tests do not establish original-game parity.
