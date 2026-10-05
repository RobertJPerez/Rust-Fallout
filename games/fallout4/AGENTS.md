> Integrated October 5 copy: the team is stopped at Robert's request. This copy uses `../../crates/fallout-data`; original pinned-revision notes below are historical. Read [INTEGRATION.md](INTEGRATION.md) and the root [closeout report](../../reports/closeout-2026-10-05.md) for current checks, commands and limits. Original source/research remain preserved. Root AGENTS.md and current stop control govern future work.

# Fallout 4 preparation workspace

This workspace was authorized separately from the active six-agent New Vegas team.
Keep `G:\Rust-Fallout` and all its worktrees read-only. Do not claim a team lane,
edit its control files, promote its branches, or duplicate its runtime systems.
Do not spawn additional agents unless Robert requests them.

Read README.md, NEXT_STEPS.md, docs/architecture.md and reports/checkpoint.json.
Use the shared `fallout-data` dependency pinned in Cargo.toml. Updates to that
revision must be deliberate, tested changes. Keep Cargo.lock committed.
Build in this workspace with a private target/cache and normally one compiler job.

Retail installations are read-only. Raw extracted assets, detailed corpus reports,
research checkouts and build output belong under ignored `local/`, never in Git.
Offline C++ reference tools are isolated from the Rust library/application.
Unknown dialects and behavior must produce explicit diagnostics. A successful
structural census does not establish gameplay, native API or save compatibility.

Maintain exact source revisions, meaningful malformed-input tests, reproducible
retail commands, and a truthful handoff. Never remove notices required by reused code.
