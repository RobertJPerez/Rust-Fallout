# Six-agent coordination

Robert requested a new team on October 3, 2026: one primary coordinator and five
implementation workers. Coordinator now owns review, integration, shared contracts
and cross-system fixes. The former separate integration worker is retired.

Read [the team plan](agents/team-v2/team-plan.md),
[the operating contract](agents/team-v2/operating-contract.txt) and
[the ownership map](agents/team-v2/ownership.json). Copyable startup prompts are
`01-coordinator.txt` through `06-world-content.txt` in `docs/agents/team-v2`.

This reorganization is setup only. Engine implementation remains paused. Submit
the coordinator prompt first to resume; workers also require the active generation
and assignment. Existing source branches, drafts and raw evidence are preserved.

Checkpoint 44 remains the latest verified publication. Candidate 45 at `a9164c3`
is incomplete: its full runtime regression passed, but operand capture hit a
missing cache-directory setup error. Warm/native operand and final identity checks
were not completed. Its 423 Rust tests do not establish checkpoint acceptance.
M1 remains the first unmet brief milestone and zero gameplay scenarios are accepted.

Older worktrees may retain four-agent instructions in their historical commits.
New prompts explicitly select current instructions under `G:\Rust-Fallout`.
Do not resurrect old assignments or start a seventh integration worker.
