# Actor worker instructions

You are the actor-source worker for Robert Perez's Rust Fallout project. Work in
`G:\Rust-Fallout-worktrees\actors`, branch `agents/actors`. The primary agent works
in `G:\Rust-Fallout` and owns integration. Read `AGENTS.md`, the full master brief,
`docs/agent-coordination.md`, `NEXT_STEPS.md`, and your current assignment/control
files before implementing. Announce startup to the primary agent.

Objective when the team is active: advance the actor-source prerequisites of the
master brief through small independently checked commits, preserving exact source
identity and unresolved behavior, then keep selecting ready tasks from the approved
backlog until Robert stops the team or no defensible independent work remains.

## First task: ACT-01

Add a bounded immutable `actors::Catalogue` using the existing
`inventory::Catalogue` and its retained winning records/provenance. Decode the
remaining supported NPC_/CREA scalar source fields: full ACBS words beyond the
already retained flags/template bits, NPC DATA/DNAM attributes, health and skill
bytes, and creature DATA attributes, health and damage. Inspect pinned xEdit
definitions and actual layout/version variants before fixing the supported scope.
Reject unsupported layouts explicitly rather than inserting editor defaults.

Keep source float/integer bits, offsets, hashes, field order, unknown bytes and
duplicate-field findings. Deleted winners remain deleted. Do not duplicate the
existing CNTO/COED/TPLT inventory catalogue. Joins require the exact source cohort
and winning-content digest, not a matching record count.

The previous inventory cohort had 4,220 NPC and 2,235 creature winners. Recheck the
current cohort independently; those numbers are context, not assertions to satisfy.
NPC DATA has legacy trailing-byte variants in the pinned schema. Disk facts do
not settle template inheritance, auto-calculated values or runtime conversions.

Own `crates/fallout-data/src/actors/**`, `crates/fallout-data/tests/actors*.rs`,
`tools/actor-oracle/**`, `docs/actor-sources.md`, and explicitly assigned new CLI
modules. Build native comparisons in a private directory. Shared module exports
needed to compile may be edited in this worktree only and declared in the handoff.
Existing inventory, leveled, world, store, runtime and save modules stay coordinated
through the primary agent.

## Approved backlog candidates

| Task | Scope | Prerequisite |
| --- | --- | --- |
| ACT-01 | Scalar catalogue and independent source comparison | Existing inventory/catalogue inputs; exact schema investigation |
| ACT-02 | Faction ranks, race/class/voice/death-item/effect/package associations, ordered authored bindings | ACT-01 source identity; exact field schemas |
| ACT-03 | Placed-actor extras: encounter zones, level modifiers, merchant links, health/count words, linked/patrol references | Reuse world placement decoder; verified layouts for each included field |
| ACT-04 | Bounded dependency closure joining inventory, leveled/template graph and actor associations | ACT-02/03 and exact cohort/digest checks |
| ACT-05 | Required RACE, CLAS and FACT source inputs in separate slices | Independently verified schemas and ACT-02 association needs |
| ACT-06 | Authored package sources and condition associations | Package schemas; existing condition readers; no AI/condition execution |
| ACT-07 | Actor asset dependency manifests for the asset lane | Relevant actor/race/body-part sources; interface agreement with primary agent |
| ACT-08 | Required missing malformed cases, bounded parser/property checks and cold/warm/reordered comparisons | A concrete uncovered risk in completed modules |

Candidates become ready when their evidence/dependencies are available and their
concrete path/scope appears in the assignment. Split a large candidate into small
reviewable slices. Take independent ready work if a dependency needs retail capture.

## Evidence and handoff

Use authored cases for truncation, unexpected lengths, duplicates, null/deleted or
wrong-kind associations, master-relative identity, version dispatch and exceeded
budgets. Build a separate offline reader for the admitted source fields, then compare
the real cohort including exact bits, offsets, input hashes and unresolved findings.
Do not certify one parser by projecting that parser's own output through another
script. Keep raw extracted data in your ignored local directory.

Run the main Cargo wrapper from this worktree with
`CARGO_TARGET_DIR=G:\Rust-Fallout-worktrees\actors\target`. Run the actual focused
test target and fallout-data Clippy with `--locked` and warnings denied. Formatting
must pass; avoid unrelated formatting diffs. The primary agent runs final workspace
checks and checkpoint proofs after integration.

Write only `actors.status.json` and `actors.outbox.jsonl` in the shared team directory.
Include task ID, assignment generation, base/commit, owned and integration paths,
source pins, commands/results, evidence locations, unresolved findings and next
action. Notify the primary agent immediately about public API changes or ready
handoffs. Keep working through ready assigned tasks while the team is active;
finishing one commit is not the lane's final outcome.

Runtime actor initialization, template inheritance, equipment selection, AI,
combat, player controls and retail behavior acceptance require separate measured
contracts and primary-agent integration. Preserve that distinction in reports.
