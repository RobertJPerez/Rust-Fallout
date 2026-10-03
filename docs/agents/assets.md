# Asset worker instructions

You are the asset-source worker for Robert Perez's Rust Fallout project. Work in
`G:\Rust-Fallout-worktrees\assets`, branch `agents/assets`. The primary agent works
in `G:\Rust-Fallout` and owns integration. Read `AGENTS.md`, the full master brief,
`docs/agent-coordination.md`, `NEXT_STEPS.md`, and your current assignment/control
files before implementing. Announce startup to the primary agent.

Objective when the team is active: advance skin, skeleton and animation prerequisites
through bounded independently checked source decoders and verified evaluation
slices, preserving authored data and unsupported branches, then keep selecting ready
tasks from the approved backlog until Robert stops the team or no defensible
independent work remains.

## First task: ASSET-01

Add exact bounded source decoding for NiSkinInstance, BSDismemberSkinInstance and
NiSkinData. Preserve transforms, bone references, vertex indices, weight bits,
dismemberment flags and every supported version branch. Type-check links, account
for shared skin owners, and report unresolved skeleton/partition dependencies.
NiSkinPartition, skin evaluation and gameplay remain unsupported in this first slice.

Read the pinned nifxml definitions and nifly Skin.cpp/Skin.hpp completely for the
admitted block layouts. Record the actual file/user/Bethesda version predicates.
The corpus contains several stream revisions; one schema cannot be assumed for
all of them. Existing NIF containers and scene links should be reused.

Important oracle constraint: the pinned nifly skin reader normalizes
`hasVertWeights > 1` to `1`. Preserve/check that raw byte independently or identify
the normalization as a comparison limitation. Reconstructed normalized values are
not proof of the original bytes. Avoid cleanup passes such as NifFile::PrepareData.
Do not silently normalize weights, reduce influences or repair source values.

Own `crates/fallout-data/src/nif_skin/**`, later `src/nif_animation/**`, corresponding
skin/animation tests, `tools/nif-skin-oracle/**`, `tools/nif-animation-oracle/**`,
dedicated build/capture wrappers, `docs/nif-skin.md` and `docs/nif-animation.md`.
New skin/animation inspector modules are allowed only when named in the assignment.
The abbreviated source paths remain under fallout-data. Minimal shared exports
needed to compile may be edited in this worktree only and declared in the handoff.
Existing NIF cursors/scene/collision modules, shared oracles, preview model wiring
and runtime/save contracts require primary-agent coordination before editing.

## Approved backlog candidates

| Task | Scope | Prerequisite |
| --- | --- | --- |
| ASSET-01 | Three skin source blocks and independent field comparison | Exact layouts/version predicates |
| ASSET-02 | NiSkinPartition palettes, maps, weights, topology and flags in bounded version slices | ASSET-01; verified partition layouts |
| ASSET-03 | Skin-root/bone binding to scene objects with authored order and graph diagnostics | ASSET-01/02; primary-agent agreement on scene access |
| ASSET-04 | First controller/sequence/interpolator/text-key source framing | Exact inherited layouts; ordered source links/timing/event fields |
| ASSET-05 | Keyframe/interpolation tags and compressed/B-spline source fields, split by supported branch | ASSET-04; independent format evidence |
| ASSET-06 | Headless sampling of verified interpolation branches | ASSET-05; independent endpoint/interior numerical cases |
| ASSET-07 | Isolated pose inspection module for one source skeleton/skin/clip | ASSET-03/06; approved preview adapter/path contract |
| ASSET-08 | Required creature/first-person/equipment attachment source coverage and negative cases | Completed slices and actor-manifest agreement |

Candidates become ready when their evidence/dependencies are available and their
concrete path/scope appears in the assignment. Source sampling does not establish
playback clocks, transitions, root motion, event delivery, face/lip behavior or
ragdoll semantics. Unsupported legacy containers need their own version path.

## Evidence and handoff

The first slice needs authored cases for null/wrong-kind links, malformed counts,
truncation/surplus bytes, bone-count mismatch, shared owners, invalid indices,
nonfinite fields, budget exhaustion and unsupported versions. Compare block spans,
hashes, integer/reference arrays and source float bits with a separate native reader.
Skinning/interpolation tolerances need a justified independent numerical contract;
retail movement or Havok units require measurements. Raw retail payloads stay local.

Run from this worktree, after the new test target exists:

```powershell
$env:CARGO_TARGET_DIR = 'G:\Rust-Fallout-worktrees\assets\target'
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked -p fallout-data --test nif_skin
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 clippy --locked -p fallout-data --all-targets -- -D warnings
```

Use private native build/output directories and explicit read-only paths to the
primary tree's pinned research sources. Never rebuild the primary agent's active
oracle or mutate its proof evidence/cache. Formatting must pass; avoid unrelated
formatting diffs. The primary agent runs final workspace checks and proofs.

Write only `assets.status.json` and `assets.outbox.jsonl` in the shared team directory.
Include task ID, assignment generation, base/commit, owned and integration paths,
source pins, commands/results, evidence locations, unresolved findings and next
action. Notify the primary agent immediately about public API changes or ready
handoffs. Continue through ready assigned tasks while the team is active and save
the exact next action before interruptions. If measured behavior blocks a task,
record it and select independent source work.
