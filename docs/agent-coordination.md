# Three-agent development plan

Robert's master brief remains the specification. The purpose of this team is to
advance independent prerequisites in parallel while preserving one coherent Rust
runtime and one verified integration history.

## Starting point

Checkpoint 42 is complete at `2de525527fc74f29e13fa7481261d0846feffa19`.
Its verified source passed 324 Rust tests and five publication checks. It adds
shared immutable runtime sources; it does not execute ObScript or establish an
accepted gameplay scenario. M1, trustworthy semantic content loading, remains the
first unmet milestone. The following milestone work can proceed on independent
prerequisites, but its acceptance still depends on the brief's gates.

Robert asked to stop after checkpoint 42, then requested this team plan. Preparing
the plan and worktrees does not resume implementation. Team control starts in
`planned` mode. Checkpoint 43 has staged, uncompiled event-preparation drafts in
`local/live-event-43-src`; the primary agent must inspect them before applying them.
Its draft counts and expected inspector exit status remain unverified.

Robert resumed implementation on October 3. Checkpoint 43 is now verified and published; the team control is active. The primary agent integrates reviewed actor and asset commits and prepares the next source-lane proof. Runtime work continues in an additional primary-owned external worktree while main is frozen. The planning history above does not override current control or Robert's latest instruction.

## Responsibilities and file ownership

| Role | Work | Owned additions | Working tree |
| --- | --- | --- | --- |
| Primary agent | Runtime, ObScript, canonical state, saves, integration and proof publication | Existing runtime/VM modules, shared entrypoints, interface reconciliation, checkpoint tooling and reports | `G:\Rust-Fallout`, branch `main` |
| Actor worker | Immutable actor records, associations and dependencies | `crates/fallout-data/src/actors/**`, `tests/actors*.rs`, `tools/actor-oracle/**`, `docs/actor-sources.md`, new actor inspector modules when assigned | `G:\Rust-Fallout-worktrees\actors`, branch `agents/actors` |
| Asset worker | Skin/skeleton/controller/animation source data, then independently verified evaluation | `crates/fallout-data/src/nif_skin/**`, `src/nif_animation/**`, corresponding tests, `tools/nif-skin-oracle/**`, `tools/nif-animation-oracle/**`, their dedicated wrappers, `docs/nif-skin.md`, `docs/nif-animation.md`, new inspectors when assigned | `G:\Rust-Fallout-worktrees\assets`, branch `agents/assets` |

The abbreviated `src/` and `tests/` paths in worker rows are under
`crates/fallout-data`. A worker does not inherit ownership of other existing files
in that crate. Each task has a concrete path list in its assignment.

The primary agent owns the final versions of `Cargo.toml`, `Cargo.lock`,
`sources.lock.json`, crate module roots, existing inventory/world/NIF readers,
CLI dispatch and evidence entrypoints, preview wiring, runtime identity/state/save
contracts, shared oracle infrastructure, README, NEXT_STEPS, parity status and
checkpoint reports. A worker may add the minimum module export or inspector
dispatch inside its isolated worktree to compile a task. It must identify those
edits as integration changes. The primary agent reconciles them instead of blindly
replacing a shared file with either worker's version.

Changes outside the assignment need a message to the primary agent before editing.
The primary agent can revise ownership without consulting Robert for routine
implementation choices. Neither worker edits the other worker's worktree.

## First tasks and rolling backlogs

The first actor task is a bounded immutable catalogue of remaining NPC_/CREA scalar
fields, using the existing inventory catalogue and its exact source provenance.
The first asset task is exact decoding of NiSkinInstance,
BSDismemberSkinInstance and NiSkinData, with typed links and an independent field
comparison. Neither task initializes gameplay actors or runs an animation system.
Detailed scope, evidence and subsequent tasks are in the two role instructions.

The primary agent first completes and verifies the staged event-preparation slice,
then follows the brief's actual unmet dependencies: explicit VM capabilities,
verified expression/condition semantics, bounded execution, runtime integration
and source/behavior comparisons. A task is selected from observed evidence rather
than from an invented sequence of future checkpoint numbers.

Workers may select the next ready task in their approved backlog while the team
is active. Dependent tasks wait for their prerequisites; waiting does not block an
unrelated source decoder, independent fixture, comparison or required coverage task.
When the backlog gets short, the primary agent adds the next dependency-ordered
slice. Preserve these responsibilities when moving to later games, with separate
version/schema adapters and the brief's New Vegas-first acceptance gates.

## Communication that survives long runs

In a coordinated session, use the agent messaging tools for immediate notices.
Direct messaging between separate Codex conversations is not assumed. Both modes
also use durable files at `G:\Rust-Fallout\local\team`:

| File | Sole writer | Contents |
| --- | --- | --- |
| `control.json` | Primary agent | Mode, run identity, stop request and resource reservations |
| `actors.assignment.json`, `assets.assignment.json` | Primary agent | Task scope, approved backlog, ownership, dependencies and assignment generation |
| `root.status.json` | Primary agent | Integration queue, current proof/freeze, verified revision and next action |
| `actors.status.json`, `assets.status.json` | Corresponding worker | Current task, branch/base/commit, changed paths, results, blockers and next action |
| `actors.outbox.jsonl`, `assets.outbox.jsonl` | Corresponding worker | Ordered notifications, proposed interfaces and handoffs |

These files stay ignored: they are mutable coordination state, not public proof.
Each status update replaces the writer's own file atomically through a temporary
file in the same directory. Each outbox is append-only with a sequence number;
acknowledgments go in the primary agent's assignment file. Never have several
agents append to a shared board or overwrite each other's status.

Read control and assignments before a task, before committing, before a long
command, and at ordinary tool boundaries. Update status at task transitions and
roughly every five minutes of active work. Send an immediate notice for an API
change, blocked dependency, failing shared invariant or ready handoff. Heartbeats
describe real work; they are not a substitute for making progress.

Missing, malformed or contradictory control/assignment state blocks implementation.
Notify the primary agent, preserve the current diff and continue only read-only
investigation until it repairs the state against Robert's latest instruction. Do
not assume an absent file means the stop request or another writer has disappeared.

The primary agent checks both outboxes while working, acknowledges handoffs and
issues follow-up work when an integrated worker finishes its turn. Workers should
continue through ready tasks within their assignment rather than return after
one small change. There is one worker per lane, even after an interruption.

## A repeatable work loop

1. Inspect the assignment, branch, actual diff and latest integration status. Read
   required references completely and record the exact version predicates.
2. Choose one bounded deliverable, announce its task ID and owned paths, and write
   the intended source/behavior contract before implementing it.
3. Implement the module with readable comments about non-obvious format or
   behavior constraints. Reuse existing source identities and catalogues.
4. Run focused tests and lint checks. Compare the real input with an independent
   reader when making a source-format claim. Preserve unresolved findings.
5. Commit an explicitly staged path list on the lane branch. Publish a handoff
   with the parent/base and commit hashes, changed paths, interface changes,
   commands/results, source pins, evidence directory, limitations and next task.
6. Read new assignments and continue a ready independent task. A merged dependency
   is incorporated only at a clean boundary under the primary agent's direction.

If a task needs an unavailable original-game measurement, save the exact scenario,
inputs, expected observation and blocker, then take independent work. Do not add
guessed defaults, no-op commands or hardcoded quest progression to make a test pass.
If every defensible task is blocked, preserve a complete handoff and report the
actual condition. Do not manufacture activity, repeat unchanged failing commands
forever, or mark an unfinished milestone complete.

## Build and evidence isolation

The worktrees are external so workers cannot enter the primary agent's source
snapshot accidentally. Their Cargo targets are respectively
`G:\Rust-Fallout-worktrees\actors\target` and
`G:\Rust-Fallout-worktrees\assets\target`. Native CMake/oracle builds and raw
reports go under each worker's own ignored `local/` directory, with a new evidence
directory for each run.

Commit these instructions before creating the worker branches, so both starting
trees contain the same protocol. Seed each assignment with the full concrete
backlog scopes and dependencies; the team mode and dependency checks still govern
which tasks may start. Creating directories/branches does not start a worker session.

Use the primary tree's Cargo wrapper from the worker's working directory. It
selects the existing local toolchain without changing the working directory:

```powershell
# Run from G:\Rust-Fallout-worktrees\actors. Use assets for the other lane.
$env:CARGO_TARGET_DIR = 'G:\Rust-Fallout-worktrees\actors\target'
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked -p fallout-data --test actors
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 clippy --locked -p fallout-data --all-targets -- -D warnings
```

The `actors` test target is a proposed new target, not an existing passing test.
Use the actual test name added by the task. The asset lane initially uses
`--test nif_skin`. Do not update the shared toolchain or dependencies casually.
Shared Cargo package caches can take locks; worker output targets must stay private.

Pinned research sources in the primary tree's `.research` are read-only inputs.
Give a worker's dedicated oracle explicit source paths and a private output/build
directory. Never run a shared build wrapper that rewrites the primary agent's live
oracle binary. Keep native research tools separate from the replacement runtime.

The primary agent schedules heavy full builds/proofs and GPU or original-game
captures. Workers can run modest focused checks in parallel, but must honor a
resource reservation before a large scan/build. Only the primary agent changes
shared extraction caches. Workers can read a completed, immutable cache or create
their own. Original installation data, saves and settings remain untouched.

## Integration and verification

Git worktrees share the repository's object database, references and configuration.
Separate paths do not make destructive Git commands safe. Workers commit only their
own branches; they do not switch/reset another branch, force-push, or change global
Git settings. The primary agent alone merges and publishes main.

For each handoff, the primary agent checks the changed paths and evidence, obtains
a read-only peer review where useful, and integrates one completed commit at a
time. It reconciles shared exports and source pins, resolves conflicts explicitly,
and runs the appropriate workspace/regression checks. Later worker commits may
depend on earlier lane commits; the handoff must identify that chain.

Full checkpoint proofs run against fixed integrated source, executables, oracles
and input hashes. No one edits those files during the proof. Workers can continue
in their external trees, but cannot alter the frozen inputs. A new integration
change needs a new proof when it affects the verification surface. Only verified
results update the public checkpoint and parity ledger. Raw retail assets never
enter Git; published summaries contain counts, hashes, scope and limitations.

## Hours of unattended work and resumption

Use an active, persistent objective for implementation, with this work loop and
the brief's evidence gates. The coordinator keeps assigning work without asking
Robert to approve routine choices. In separate sessions, use each role's objective
as that session's Goal when available; an ordinary one-off prompt can finish and
wait. Plan-only work does not itself activate an implementation objective.

[Official OpenAI documentation](https://developers.openai.com/cookbook/examples/codex/using_goals_in_codex)
describes Goals continuing while active, subject to completion, pause,
interruption, budgets and blockers. The design supports hours of useful work and
resumption. It cannot guarantee infinite runtime or override account limits,
session termination, unavailable services, or an offline machine. Keeping a
durable backlog cannot make a stopped process run.

Before compaction or interruption, save the exact task, actual branch/diff,
base/commit hashes, last successful and failing checks, immutable evidence paths,
unmerged handoffs, blocker and next command. On resumption, compare that record
with the filesystem and control state; do not rerun already completed work or
assume staged drafts passed. The primary agent wakes an idle worker or replaces
a lost session only after confirming no other writer still owns that lane.

Robert's stop/pause instruction propagates to both workers immediately. Preserve
work safely and start no new task. A request to finish the next checkpoint becomes
one explicit integration boundary, with workers quiescing before its final proof.
