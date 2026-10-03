# Working together on Rust Fallout

The current arrangement is one coordinator plus five implementation workers.
Read `G:\Rust-Fallout\docs\agents\team-v2\team-plan.md` and
`G:\Rust-Fallout\docs\agents\team-v2\operating-contract.txt` before work.
Current instructions and assignments live in the primary tree even when a worker
checkout still contains an older four-agent plan.

- Robert's latest instruction controls. Setup/planning does not resume engine
  implementation. Read `G:\Rust-Fallout\local\team\control.json` and your
  `G:\Rust-Fallout\local\team-v2\<lane>.assignment.json` before edits,
  commands, commits and task transitions. Require matching generation, active
  control and active assignment. Missing/malformed coordination permits only
  read-only investigation until the coordinator repairs it.
- Read the entire master brief, NEXT_STEPS.md, latest verified checkpoint and
  preserved handoffs. Source decoding and engineering tests do not prove gameplay.
- Use the assigned worktree, branch and private Cargo/native/evidence directories.
  All sessions share the filesystem, Git object database, refs and caches.
  Do not reset, clean, rebase, remove or overwrite another lane's work.
- Follow the ownership map and narrower task assignment. Coordinator owns shared
  exports, dependencies, source pins, public contracts and final promotion.
  Minimal private export/CLI wiring is allowed when declared. Coordinate semantic
  APIs, canonical state and save formats before changes. Ask the coordinator,
  not Robert, to resolve routine engineering decisions.
- One session owns a lane. Claim its lease before source/mailbox writes; never
  steal an existing lease. Workers write only their own status/outbox. Coordinator
  writes assignments and the integration board. Separate chats do not share
  messages automatically: read the durable mailboxes.
- Keep one active task and at most two new unmerged handoffs per worker unless
  explicitly reassigned. Prioritize review fixes and dependency integration.
  Deliver reachable functionality; do not create duplicate parsers or empty crates.
- Original installations, saves/settings and pinned research remain untouched.
  Never rebuild binaries used by a live proof. Raw retail material stays local.
- Only coordinator assigns checkpoints and promotes/pushes main. Interrupted or
  failed proofs remain incomplete. Earlier scoped reports stay immutable.
- STOP means stop immediately at a safe cancellation boundary, preserve work and
  cancel only owned processes. Do not start another fix, test, commit, proof or
  publication afterward. Check control during long commands, not after a whole
  milestone. Resume only on Robert's instruction and reconciled assignments.

The operating contract defines startup, handoff, build scheduling and resumption.
Historical four-agent role files are superseded by the team-v2 startup prompts.
