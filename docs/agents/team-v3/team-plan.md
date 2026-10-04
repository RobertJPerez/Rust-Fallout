# Eight agents building one usable engine

Prepared October 4, 2026 UTC for Robert Perez. This replaces the six-agent team.
Setup is paused. Start Agent 1 first, then the seven worker sessions. Submitting
the coordinator prompt authorizes activation; preparing these files does not.

The immediate product target is a source-driven New Vegas route with visible
actors, collision, scripts, dialogue consequences and cold Continue. NV and each
official DLC remain the priority. FO3, TTW, FO4, measured FO76 and the optional
Starfield/crossover work remain in the full brief; no agent is assigned a whole
later game before the shared NV dependencies work.

| Agent | Lane | VS Code folder | Concrete first contribution |
| --- | --- | --- | --- |
| 1 | Coordinator | `G:\Rust-Fallout` | Integrate, fix shared interfaces, build isolated retail measurement infrastructure and prove batches |
| 2 | Scripts | `G:\Rust-Fallout-worktrees\v3-scripts` | Executable semantic probes and source-driven execution admission, then measured VM effects |
| 3 | Runtime | `G:\Rust-Fallout-worktrees\v3-runtime` | Remaining save-rotation validation and persistent reference state consumed by the application |
| 4 | Actors | `G:\Rust-Fallout-worktrees\v3-actors` | Actor render dependencies and package/context adapters, then measured actor rules |
| 5 | Assets | `G:\Rust-Fallout-worktrees\v3-assets` | Source skeleton/skin pose evaluation consumed by rendering |
| 6 | World | `G:\Rust-Fallout-worktrees\v3-world` | Connect existing resource jobs to actual cell residency and resolve profile precedence |
| 7 | Presentation | `G:\Rust-Fallout-worktrees\v3-presentation` | Real source poses on screen, responsive loading, input, then original UI/audio |
| 8 | Physics | `G:\Rust-Fallout-worktrees\v3-physics` | Authored collision queries and source navigation, then constrained movement |

Agent 1 also exclusively owns `G:\Rust-Fallout-worktrees\integration` on
`agents/team-v3-base`. Old worker trees are preserved archives of their lane's
history and private evidence. Do not reopen them as competing implementation
sessions. The new branches are `agents/v3-<lane>`. They share one reviewed
development base; that base is not a new accepted gameplay checkpoint.

## What changed and why

The old world lane included rendering, input, collision and future narrative
systems. Its implemented resource jobs currently feed inspection commands; the
application does not consume them. Presentation and physics are now separate
owners with actual consumers. Asset sampling must lead to a visible pose;
collision decoding must lead to a query and movement. Another report of the same
decoded sources does not substitute for either.

The original/replacement behavior measurement gap is explicit work: coordinator
owns isolated profile/launch/capture infrastructure; scripts owns script fixtures
and semantic traces; physics owns units/contact fixtures; world owns lookup
precedence; presentation owns matched cameras/input/audio captures. A missing
original-game result blocks that claim, not unrelated engineering implementation.

Each lane has at least three initially ready tasks in [backlog.json](backlog.json),
plus dependent work. Each task names its real consumer and acceptance. Ready
means work can start using the current base and assigned files; it does not mean
retail acceptance is already possible. If a capture/API/resource is unavailable,
record the exact blocker and take another ready task. Do not repeatedly run an
unchanged failing experiment or manufacture a new inspector just to stay busy.

## Start and keep moving

1. Agent 1 verifies the preserved base, local proof receipt and absence of active
   old writers, then creates one fresh run ID. It activates assignments with ready
   queues before publishing active global control. Start the workers afterward.
2. Each worker claims its exclusive lane lease, announces its task and advances
   the ready queue without asking for a grant. Workers mark completed tasks in
   their own status/outbox; coordinator reconciles the board. A seed backlog is
   not a command to redo an already completed task.
3. Public contracts in [contracts.json](contracts.json) authorize the initial
   state/residency/pose/query boundaries. Private helpers and additive consumer
   adapters within those contracts need no repeated approval. Shared persistent
   semantics, dependencies or ownership transfers need a written decision, while
   the worker continues a separate ready task.
4. Builds use automatic machine slots. The nonblocking default returns exit 75
   when busy, allowing useful source work instead of a session waiting on a lock.
   Keep one coding slice plus at most one slice awaiting validation. Test before
   handing off; never call a busy response a passing or failing test.
5. Send small tested commits early. Coordinator reviews actionable handoffs at
   task/command boundaries and before starting another long proof. Publish
   reviewed development bases after combined checks; workers need not wait for a
   numbered checkpoint to consume a reviewed dependency. Preserve original-to-
   integrated commit mapping so prerequisite stacks are not replayed.
6. If four unmerged handoffs accumulate, finish fixes/review in that stack and
   send an integration-priority message. Coordinator handles that lane next.
   Independent measured consumer work may continue; do not grow an unlimited
   dependent branch. There is no instruction to waste credits polling.

## Coordinator work is engineering work

The coordinator's ordered queue is in the same backlog as the workers. It repairs
shared joins and wiring, implements isolated baseline/capture tooling, integrates
worker code, runs combined checks, prepares scoped acceptance and fixes failures.
It delegates features in owned worker modules instead of competing with workers.
It checks proposals before launching another long check and records a decision or
exact unmet prerequisite, with an independent worker task already assigned.

Freeze a proof in a separate coordinator-owned worktree with immutable sources,
binaries and input hashes. During that proof, the integration checkout can review
the next batch. Do not rebuild a frozen proof target. One new failing setup does
not justify rerunning every corpus proof: smoke-test directories, provenance,
arguments and intended diagnostics first.

## Resources and honest progress

The setup-time host reported about 40 GiB total RAM and only 5 GiB free. Start with
one focused build and one heavy/GPU/retail operation, not eight simultaneous Cargo
builds. Cargo defaults to two jobs per worker; use one when memory pressure is
observed. No coordinator grant is needed for either slot. Heavy users state the
purpose in status. Concurrent commands still obey total resources and proof
isolation; do not increase concurrency without measuring memory and responsiveness.

The wrapper enforces active generation, run, caller lease, branch, worktree and
private output target. STOP propagates through global control and current-run
outboxes. Old generation stop history remains preserved. Service/session limits
and real missing dependencies can still end or block a session. The policy is
continuous useful work while authorized, not a promise of infinite execution.

## Scope after the first queues

Keep a rolling next-task list, selecting observed gaps on the player route:

- Scripts: native semantics, event lifecycle, conditions, quest/dialogue effects,
  resumable execution; separate later Papyrus dialect when justified.
- Runtime: reference/actor/quest components, clocks, continuations and migrations,
  native durability; retail saves and cross-profile transfers remain separate.
- Actors: initialization, packages/navigation consumers, combat/equipment/effects,
  progression, faction/crime and companion rules.
- Assets: skins, sampled clips, transitions/events, attachments, first-person and
  creature rigs, face/lip behavior and source-preserving caches.
- World: profile/VFS, streaming, doors, narrative and quest source selection,
  terrain/water/weather sources, content coverage and later game adapters.
- Presentation: rendering/materials, both input methods, original XML/tile menus,
  dialogue/voice/subtitles, HUD/Pip-Boy, audio/media and accessibility.
- Physics: static/dynamic shape queries, movement, nav/portals, obstacles,
  projectiles/contact/ragdoll adapters with observed behavioral limits.
- Coordinator: cross-system integration, baseline/proof infrastructure, shared
  sources/contracts, end-to-end route acceptance, release/performance gates.

These are future directions, not permission to implement guessed rules or create
empty modules. Reassign ownership explicitly at clean boundaries when evidence
changes the critical path. No file silently acquires two writers.

## Preservation and evidence

[preserved-work.json](preserved-work.json) contains the exact base, six mapped
commits and five preserved handoff receipts. The complete pre-v3 Git bundle was
verified. All ten preexisting worktrees were clean and their heads protected by
preservation refs. Raw evidence remains at its original paths, not inside the
bundle. The old proof46 tree remains unbuilt and unchanged.

[review.md](review.md) separates code findings from architectural gaps and static
reasoning from executed tests. Checkpoint 45 remains the last published verified
checkpoint. M1 is unmet and zero gameplay scenarios are accepted. Worker tests,
development integration and retail acceptance are separate levels of evidence.
