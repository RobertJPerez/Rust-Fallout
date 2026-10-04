# Working together on Rust Fallout

Read [the eight-agent plan](docs/agents/team-v3/team-plan.md),
[operating contract](docs/agents/team-v3/operating-contract.txt),
[ownership map](docs/agents/team-v3/ownership.json),
[preauthorized interfaces](docs/agents/team-v3/contracts.json) and
[concrete backlogs](docs/agents/team-v3/backlog.json).
Robert requested one coordinator and seven implementation workers. The coordinator
owns review, integration, shared engineering, original-profile infrastructure and
main promotion. Workers own scripts, runtime, actors, assets, world, presentation
and physics. There is no separate integration agent.

- Robert's latest instruction overrides coordination files. The prepared v3 team
  is paused until its coordinator startup prompt is submitted and a fresh run is
  activated. Setup/review authorized directly by Robert may proceed while engine
  implementation remains paused. Old v2 writers stay stopped.
- Read the entire master brief in docs/references/Fallout_Rust_Codex_Master_Brief.txt,
  NEXT_STEPS.md and the latest verified checkpoint. Decoding and inspection do not
  establish gameplay parity. Read relevant pinned source fully before deriving rules.
- Before commands, edits, commits and task transitions, check
  G:\Rust-Fallout\local\team\control.json and your v3 assignment. Missing/malformed
  control or assignment permits read-only investigation until repaired. Old checkout
  instructions do not supersede these current central instructions.
- Use your assigned external worktree, branch, private targets and evidence. All
  sessions share filesystem/Git refs; no duplicate writers. Claim an exclusive
  current-run session lease. Never overwrite a live lease or expire it by age.
- Edit only assigned paths. Published contracts preauthorize their bounded producer/
  consumer changes. Minimal private module export/CLI wiring is allowed if declared
  in handoff. New dependencies, source pins or shared semantics outside the contracts
  require a coordinator decision; proceed with an independent ready task meanwhile.
- Each worker writes only its status/outbox; coordinator owns assignments/board/
  control. Announce startup, interface proposals, blockers and tested handoffs.
  Read other current-run outboxes for STOP; separate chats have no assumed live bus.
- While active, select the next ready task yourself. Backlogs contain concrete
  consumers, acceptance and dependencies. If one is blocked, take independent work.
  If the queue is exhausted, derive a bounded useful task within ownership from the
  brief, record its consumer/exit and proceed. Do not wait for a per-task grant or
  repeat completed audits. One coding task plus at most one parked validation slice.
- Use tools/team-build.py for focused builds and heavy/GPU/proof work. Admission
  is automatic; default busy exit75 launches nothing, so do other useful work.
  One focused plus one heavy operation initially. Do not bypass limits or rebuild
  frozen proof binaries. Keep installation, saves, settings and research read-only.
- Send small tested handoffs with exact commits, dependency mapping, commands,
  results, evidence hashes, shared wiring and next action. Review corrections take
  priority. At four unmerged handoffs request integration priority and improve that
  stack rather than growing an unlimited dependent branch. Coordinator publishes
  checked development dependencies before the next checkpoint when appropriate.
- Only coordinator promotes/pushes main and assigns checkpoints. Do not reset,
  clean, rebase, remove or force-push another branch/tree. Preserve all prior drafts,
  handoffs and evidence. Raw retail data must not enter public Git artifacts.
- On interruption preserve task/head/base/draft/tests/evidence/next action. On STOP,
  propagate immediately; start no new task, cancel only your own processes, preserve
  state and return. An explicitly graceful stop has only the user's stated boundary
  and deadline. No queued work or persistent Goal overrides STOP.

Copyable prompts: docs/agents/team-v3/01-coordinator.txt through 08-physics.txt.
Old team-v1/v2 files and worktrees remain preserved historical records.
