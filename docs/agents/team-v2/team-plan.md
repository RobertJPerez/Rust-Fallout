# Six agents, one engine

Robert requested one coordinator and five implementation workers on October 3,
2026. The coordinator owns review, integration and shared decisions. Workers
advance independent prerequisites toward the same source-driven New Vegas route.
More workers should shorten that dependency path, not multiply inspectors/proofs.

## Launch

Setup leaves engine implementation paused. Submit `01-coordinator.txt` to the
main session first, then each numbered worker prompt in its assigned folder.
The submitted coordinator prompt explicitly requests resumption and activation
of generation `team-v2-20261003`. Workers also require active matching assignments.
Do not launch another coordinator or a seventh integration worker.

| Agent | Responsibility | VS Code folder | Branch |
| --- | --- | --- | --- |
| 1 Coordinator | Review, integration, contracts, shared fixes, publication | `G:\Rust-Fallout` | `main` |
| 2 Scripts | ObScript, conditions, native capabilities and execution | `G:\Rust-Fallout-worktrees\script-vm` | `agents/script-vm` |
| 3 Runtime | Canonical state, identity, events, inventory and saves | `G:\Rust-Fallout-worktrees\runtime-next` | `runtime/operand-probes` |
| 4 Actors | Actor/package inputs, dependency closure and verified rules | `G:\Rust-Fallout-worktrees\actors` | `agents/actors` |
| 5 Assets | Skin, skeleton, controllers, keyframes and animation | `G:\Rust-Fallout-worktrees\assets` | `agents/assets` |
| 6 World | Content resolution, world/cells, streaming and presentation | `G:\Rust-Fallout-worktrees\world-content` | `agents/world-content` |

Agent 1 also owns `G:\Rust-Fallout-worktrees\integration`, branch
`agents/integration`, for candidate edits/builds/proofs. Main is the promotion
tree. Both folders have one writer: the coordinator; the old integration session
is retired. Six separate sessions are intended, not already-running processes.
Separate chats use durable mailboxes; automatic cross-chat messaging is not assumed.
Worktrees separate checked-out files while sharing Git refs and caches. See the
[official worktree guidance](https://learn.chatgpt.com/docs/environments/git-worktrees).

## Preserved work

Backup: `G:\Rust-Fallout-preservation\team-v1-20261003`. Its manifest records
heads, dirty bytes, evidence locations and hashes. `all-refs.bundle` was verified
and contains all pre-reorganization Git refs/history. Preservation branches under
`preserved/team-v1-20261003/` protect every old head. Raw evidence/build output and
research remain in their original directories; they were not moved or deleted
and are not represented as copied into the small Git bundle.

| Lane | Preserved head | Meaning |
| --- | --- | --- |
| main | `63d2173f4a0a3e7fd7c70db105733ab153ddcbe2` | Verified checkpoint 44 |
| integration | `a9164c3bedeb9d72bd2520b7d9b2a957e90a61b7` | Incomplete candidate 45 |
| runtime-next | `794d680392d8c569c64edb6206844bf0d2c3855b` | Tested cache/save worker, outside integrated acceptance |
| actors | `d4c15d41c0fa725a439102be1ceb84eb3f6f8d3e` | Linked-reference/RACE followups beyond 45 |
| assets | `1635589be353f5aaab52c71d906e0939e057d3b5` | Tested ASSET-04; three untested ASSET-05A files |

The asset draft consists of `nif_animation/mod.rs`, `keyframe/mod.rs` and
`keyframe/read.rs` under `crates/fallout-data/src`. Preserve those bytes before
continuing. The original receipt is
`G:\Rust-Fallout-worktrees\assets\local\asset-05a-dev-20261003-01\stop-preservation-01\interruption.json`.
It does not claim the draft compiles or passes tests.

Scripts starts from runtime head `794d680`; world starts from verified main
`63d2173`. Scripts inherits unmerged cache `2069565` and save-worker `794d680` as
development dependencies, not accepted features. The coordinator imports each
dependency once and maps original to integrated commits. Existing worktrees keep
their heads during setup. Absolute-path current instructions in the primary tree
supersede historical team instructions still present in older worker checkouts.

## First tasks and rolling backlog

1. Coordinator: adopt the stopped candidate and paused handoff. Checkpoint 45 is
   incomplete: its full runtime regression passed, but the cold operand capture
   failed because the runner omitted its cache directory. Fix that setup and
   smoke-test the actual path/diagnostic before another expensive run. Reconcile
   current main documentation in the candidate, commit, then complete a fresh
   proof of the fixed 45 scope. Keep later cache/save/RACE/animation work outside it.
2. Scripts: inherit programs, preparation and operand observations. Propose and
   implement a bounded native capability/dispatch boundary with a real source-plan
   consumer, typed unsupported outcomes and explicit contexts. Then establish one
   expression/branch/native semantic at a time with independent evidence, followed
   by bounded execution, continuations and condition evaluation. Source descriptors
   and engineering GetItemCount arithmetic do not prove retail execution.
3. Runtime: adopt the completed save-worker handoff, then close the demonstrated
   snapshot admission gap: bound current/legacy JSON collections before allocating
   owned state. Preserve escaped keys, positional structs, strict validation and
   migrations. Next provide narrow staged mutation, revision validation and event
   acknowledgment APIs for scripts. Add saved state only with a real consumer and
   restoration test; do not invent actor defaults or VM scheduling.
4. Actors: review old outbox 38 and finish its PACK contract with the coordinator.
   First slice: exact PKDT 8/12-byte and PSDT 8-byte fields, original order,
   missing/duplicate findings, bounds and independent authored/retail comparisons.
   Reuse script-owned CTDA/embedded-script decoders. Continue actor asset dependency
   manifests, then measured initialization/rules through canonical runtime APIs.
5. Assets: continue the preserved NiTransformData draft. Retain quaternion,
   scalar/vector/XYZ groups, tags, counts and exact float bits. Complete independent
   comparisons and schema-1 regression before claiming ASSET-05A delivered. Continue
   bounded compressed/B-spline branches, headless sampling and an agreed pose
   consumer. Do not silently sort keys, repair weights or invent event timing.
6. World: add bounded cancellable execution to existing preparation/cache jobs,
   with generation tokens and one reachable consumer. Test stale completion,
   interruption/retry and publication boundaries; do not create a second importer.
   Track M1 exceptions and profile lookup precedence, then winning CELL/WRLD
   dependencies, streamed interior/exterior, collision and both input methods.
   Unknown retail precedence or units remain explicit rather than guessed.

These are bounded starting tasks and future directions, not permission to build
all modules at once. Assignments name exact paths, prerequisites, evidence and
exit conditions. No empty future-game crates or architecture rewrites.

## Ownership and interfaces

`ownership.json` is the explicit path map. Some paths reserve future modules;
they are not claims of existing implementation. Assignments can narrow these
paths. The coordinator must record any ownership transfer before edits.

- Scripts owns `programs.rs`, VM-specific preparation/operand adapters, query
  routing and conditions. Runtime owns mutable banks, event storage, identity,
  revisions and save encoding. Interface changes involve both owners and the
  coordinator; neither creates another World or event journal.
- Actors owns actor-specific extras; world owns generic placement decoding.
  Actor rules use runtime APIs. Package conditions reuse the single CTDA owner.
- Assets owns skin/animation decoding and evaluation. World owns scene assembly,
  `preview/model.rs`, rendering and input. Pose wiring gets a named adapter and
  agreed interface before either worker changes presentation.
- Narrative/dialogue source selection belongs to world; embedded scripts and
  conditions remain scripts. Later quest/dialogue runtime tasks are explicitly
  assigned as prerequisites close. They remain part of the full brief.
- Shared store/plugin/index/identity code, dependencies, source pins, crate roots,
  CLI/evidence dispatch and public reports stay with the coordinator unless an
  individual task explicitly delegates paths. Minimal private export/CLI wiring
  is allowed and must be declared, not a license for concurrent shared redesign.

Contract proposals state producer/consumer, types/paths, identity, budgets,
errors/unsupported cases, mutation order and save impact. The coordinator decides
routine engineering questions without asking Robert. Private helpers do not need
repeated permission. No consumer writes around canonical state validation.

## Review throughput and proof scheduling

Keep one active task and at most six new unmerged handoffs per worker. Preserved
backlogs are grandfathered, but review them before adding another dependent stack.
After each handoff, take the next ready assignment task yourself. If its
dependency is missing, record the blocker once and take an independent owned
slice. When the listed queue is exhausted, derive the next source-backed task
from the master brief and NEXT_STEPS.md within your owned paths, name its real
consumer and small tested exit, and publish it in your own outbox. Do not wait
for a coordinator task signal. At the handoff cap, review/fix existing handoffs
and investigate source evidence while coordinator integrates; do not create an
unbounded branch or manufacture busywork. Send small tested commits early.

Coordinator reviews mailboxes at command/task boundaries and routes production
defects to owners. A small cross-system candidate fix is allowed after recording
the paths and notifying affected owners. Workers consume that fix before editing
the behavior again. No competing implementation and no blind merge from main.

Workers run focused tests, formatting and affected-package Clippy. Coordinator
runs combined checks and one full proof for a frozen reviewed batch. Cheap setup
checks come first: directories, provenance, arguments, intended diagnostics and
private output locations. Repeat a full proof only for a new relevant change,
failure or unresolved concern, not every status request or worker commit.

Permit one heavy full proof/GPU/retail capture and one focused Cargo/native build
concurrently, with two Cargo jobs per focused build. Coordinator reserves the
heavy slot and adjusts limits from measured resources. Workers use the automatic
focused-slot wrapper in the operating contract; it serializes their commands
without a coordinator grant. Read-only review and private
implementation can continue while candidate source/binaries/inputs are frozen.

Preserve historical reports and raw failed attempts. Code decoding, engine-state
guarantees, retail comparisons and gameplay acceptance are separate claims.
Checkpoint 44 is the latest verified checkpoint; M1 is still unmet and zero
gameplay scenarios are accepted. A test count is not a completion percentage.

## Long runs, later work and stopping

All lanes target the dependency graph toward an authentic source-driven route:
VM effects, visible actors/collision, dialogue/quest consequences and cold Continue.
Reassign workers to UI/audio, dialogue, combat/AI and content coverage as earlier
gates close. NV base/DLC first; retain the full FO3/TTW/FO4/FO76/crossover roadmap.
Do not allocate one agent per game now.

The operating contract defines durable handoffs and resumption. An immediate STOP
overrides a pending checkpoint/test/upload. All sessions check global control and
all current-run stop messages during short process polls, so a coordinator running
a long proof does not delay propagation. Cancel only owned processes, save the actual draft/evidence/next action and
return. Do not wait for a whole proof to finish before noticing a stop. Repeated
failures are preserved/reported rather than used to extend a stop indefinitely.
Unavailable original-game measurements block their acceptance claims; workers
can still finish independent work when Robert has authorized active implementation.
