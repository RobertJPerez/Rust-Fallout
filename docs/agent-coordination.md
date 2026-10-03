# Four-agent development plan

Robert's master brief remains the specification. The purpose of this team is to
advance independent prerequisites in parallel while preserving one coherent Rust
runtime and one verified integration history.

## Starting point

Checkpoint 44 verifies actor scalar sources and the first three skin source
blocks alongside the full runtime/source regression: 359 Rust tests and five
publication checks. Source readers and explicit engineering state do not establish
gameplay parity. M1, trustworthy semantic content loading, remains the first unmet
milestone. Independent prerequisites can proceed while acceptance follows the brief.

Robert resumed implementation on October 3 and asked for a fourth review/integration
agent. Team control is active. Runtime work continues in a primary-owned external
worktree, and the integration worker prepares candidates and proofs separately.
The current control and Robert's latest instruction govern continuation.

## Responsibilities and file ownership

| Role | Work | Owned additions | Working tree |
| --- | --- | --- | --- |
| Primary agent | Runtime, ObScript, canonical state, saves, semantic contracts, final main promotion/push | Runtime/VM modules and final shared contracts | `G:\Rust-Fallout`, branch `main`; scripting in `G:\Rust-Fallout-worktrees\runtime-next` |
| Actor worker | Immutable actor records, associations and dependencies | `crates/fallout-data/src/actors/**`, `tests/actors*.rs`, `tools/actor-oracle/**`, `docs/actor-sources.md`, new actor inspector modules when assigned | `G:\Rust-Fallout-worktrees\actors`, branch `agents/actors` |
| Asset worker | Skin/skeleton/controller/animation source data, then independently verified evaluation | `crates/fallout-data/src/nif_skin/**`, `src/nif_animation/**`, corresponding tests, `tools/nif-skin-oracle/**`, `tools/nif-animation-oracle/**`, their dedicated wrappers, `docs/nif-skin.md`, `docs/nif-animation.md`, new inspectors when assigned | `G:\Rust-Fallout-worktrees\assets`, branch `agents/assets` |
| Review/integration worker | Review handoffs, reconcile shared wiring, run fresh proofs, prepare candidate publication | Candidate shared exports/dispatch, integration verification, approved source-pin reconciliation, `docs/integration/**`, candidate reports/parity | `G:\Rust-Fallout-worktrees\integration`, branch `agents/integration` |

The abbreviated `src/` and `tests/` paths in worker rows are under
`crates/fallout-data`. A worker does not inherit ownership of other existing files
in that crate. Each task has a concrete path list in its assignment.

The primary agent owns the final contracts for `Cargo.toml`, `Cargo.lock`,
`sources.lock.json`, crate module roots, existing inventory/world/NIF readers,
CLI dispatch and evidence entrypoints, preview wiring, runtime identity/state/save
contracts, shared oracle infrastructure, README, NEXT_STEPS, parity status and
checkpoint reports. The integration worker prepares tested candidate versions;
runtime/API/dependency decisions remain coordinated with primary. A worker may add the minimum module export or inspector
dispatch inside its isolated worktree to compile a task. It must identify those
edits as integration changes. The integration worker reconciles them instead of blindly
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
Detailed implementation backlogs are in the actor and asset instructions; the fourth role continuously reviews their handoffs and prepares tested integration candidates.

The primary agent follows the brief's actual unmet dependencies: explicit VM capabilities,
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
| `integration.assignment.json` | Primary agent | Review queue, approved base, proof scope/checkpoint reservation and integration ownership |
| `root.status.json` | Primary agent | Integration queue, current proof/freeze, verified revision and next action |
| `actors.status.json`, `assets.status.json` | Corresponding worker | Current task, branch/base/commit, changed paths, results, blockers and next action |
| `actors.outbox.jsonl`, `assets.outbox.jsonl` | Corresponding worker | Ordered notifications, proposed interfaces and handoffs |
| `integration.status.json`, `integration.outbox.jsonl` | Integration worker | Review dispositions, candidate chain, proof freeze, results and tested handoffs |

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

The integration worker reviews implementation outboxes and prepares tested candidates.
Primary checks all outboxes, acknowledges handoffs, routes defects/contracts and
reserves checkpoint numbers. Workers should
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
`G:\Rust-Fallout-worktrees\assets\target`. The integration worker uses
`G:\Rust-Fallout-worktrees\integration\target`. Native CMake/oracle builds and raw
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

For each handoff, the integration worker reviews complete changed files and
evidence, requests bounded owner fixes, then applies reviewed commits in its
candidate tree. It reconciles shared exports and approved pins, resolves conflicts
explicitly, and runs the appropriate workspace/regression checks. Later commits
may depend on earlier lane commits; every handoff identifies that chain. Primary
promotes a tested candidate at a clean boundary and pushes main. A complete proof
is repeated only after a new change or an unresolved concern.

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

Robert's stop/pause instruction propagates to all workers immediately. Preserve
work safely and start no new task. A request to finish the next checkpoint becomes
one explicit integration boundary, with workers quiescing before its final proof.
