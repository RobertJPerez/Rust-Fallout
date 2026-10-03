# Working together on Rust Fallout

Read [the team plan](docs/agent-coordination.md) before parallel implementation.
Robert asked for two workers alongside the primary agent. The primary agent owns
integration; workers own the actor and asset lanes described in that plan.

- Use the assigned external Git worktree. All agents share the filesystem; spawning
  an agent does not isolate its files, binaries, Git references or evidence.
- Read the entire master brief in `docs/references/Fallout_Rust_Codex_Master_Brief.txt`
  before implementation. Keep `NEXT_STEPS.md` and the latest verified checkpoint in
  view. Source decoding and inspector output do not establish gameplay parity.
- Check `G:\Rust-Fallout\local\team\control.json` and your assignment before starting
  a task and at task boundaries. A planning or paused team does not authorize a
  worker to restart implementation. The primary agent reconciles this control file
  with Robert's latest instruction; the file never overrides Robert.
  Missing or malformed coordination files are a blocker to implementation; report
  them to the primary agent and continue only read-only investigation until repaired.
- Edit only your assigned paths. Minimal shared-file wiring in your own worktree
  is allowed when declared in the handoff. Public API, dependency, runtime-state or
  save-format changes need coordination with the primary agent before implementation.
- Tell the primary agent when starting a task, proposing an interface change,
  finding a blocker, or handing off a tested commit. Write only your own status and
  outbox files. The primary agent writes assignments and the integration board.
- Keep Cargo targets, native build directories and raw evidence separate. Read the
  installation and pinned research sources without changing them. Never reuse a
  primary-agent proof directory or rebuild a binary used by a live proof.
- Only the primary agent merges and pushes `main`, assigns checkpoint numbers,
  updates the source lock/parity ledger, and publishes verification receipts.
  Do not reset another branch, change global Git configuration, or force-push.
- Complete small, reviewable tasks, run relevant checks, save a handoff, and take
  the next ready task in your lane while the team is active. Record blocked tasks
  precisely and continue independent work. Never invent behavior to clear a gate.
- On interruption, preserve the current task, commit/base revisions, test results,
  evidence paths and next action. On resumption, inspect the real working tree and
  reread control/assignment state before continuing. Propagate Robert's stop request
  to the whole team; do not start another task after it.

Role instructions: [actors](docs/agents/actors.md), [assets](docs/agents/assets.md).
