> Integrated October 5 copy: the team is stopped at Robert's request. This copy uses `../../crates/fallout-data`; original pinned-revision notes below are historical. Read [INTEGRATION.md](INTEGRATION.md) and the root [closeout report](../../reports/closeout-2026-10-05.md) for current checks, commands and limits. Original source/research remain preserved. Root AGENTS.md and current stop control govern future work.

# Skyrim preparation

Work only in this repository. Robert has six agents actively working on the
separate New Vegas project. Read its current code and handoffs before proposing
shared work; do not edit its working trees, leases, mailboxes, dependencies or
research checkouts from this project.

Use the pinned `fallout-data` dependency and its archive backend. Do not fork
record framing, decompression, asset-path normalization, source locking or hashing.
The Fallout 4 preparation project also has Papyrus bytecode work; inspect it before
adding any PEX decoder or VM. Skyrim VMAD is this project's on-disk attachment adapter.

Keep original game data read-only. Retail files and generated corpus reports stay
under ignored local paths. Use synthetic distributable fixtures in tests. Match
actual executable/content versions rather than marketing labels. Never treat the
presence of Creation filenames as an ownership check or complete AE content set.

Use private Cargo caches and targets, normally one build job while the NV team is
busy. Test, format and lint changed Rust code. Unknown formats/behavior must remain
visible; no successful no-ops or fabricated campaign acceptance. Do not expand the
shared ProfileId enum indirectly by labeling Skyrim as Fallout or crossover.
