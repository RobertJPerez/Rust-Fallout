# Reorganization review, October 4, 2026 UTC

Reviewed the original 2,058-line revision-3 brief in full, current coordination
documents, NEXT_STEPS, checkpoint45, candidate3835, five stopped handoff receipts
and the affected script/save/actor/asset/world/presentation paths. Three read-only
reviews covered execution/state, visual/world systems and coordination tooling.
This is a targeted review, not a claim that every repository file or every source
atlas repository has been audited.

Original and repository brief copies are byte-identical:
`540f544a41d76388e30f80af939e045f95e2a6310e4df2b047e60cd4e4efc76e`.
The atlas remains supporting research with per-source CODE/DOC/META distinctions;
deferred references in sources.lock.json have not become validated dependencies.

## Correctness findings and disposition

| Priority | Finding | Evidence and disposition |
| --- | --- | --- |
| P1 | Candidate save rotation could replace a valid previous save with a checksum-valid current save containing invalid intrinsic links. | `fallout-runtime/src/save/repository.rs` preflight; existing STATE04K reproductions and fix `e6b4864` retained and mapped to `2ed84c7`. Do not rediscover it. |
| P1 | Quest declaration join could accept headers from a different source snapshot. | `fallout-data/src/quest_scripts.rs` attachment validation; VM03E `a648fff` checks source digest before declaration selection, mapped to `4a94adb`, with authored regression. |
| P2 | A source-schema-invalid current save may still rotate over a valid previous save. | Intrinsic validation does not check declaration kind/event site; restore does. Static path: `snapshot/relationships.rs`, `snapshot.rs`, `save/repository.rs`. STATE04K explicitly leaves this boundary. STATE01 must reproduce a checksum-valid numeric-local-to-reference change and close it with bounded source context. No new executed reproduction is claimed here. |
| P2 | Source camera validation can accept an overflowing direction and display the wrong facing. | `fallout-preview/src/scene.rs:298`: finite endpoints can subtract to infinity; subsequent NaN comparisons evade rejection. Pinned Bevy0.19.1 look-to fallback substitutes NEG_Z. Static source trace, not a reproduced crash. VIEW01 owns the regression/fix. |
| P2 | Old build wrapper rejects UTF-8-BOM status and can accept a replaced session lease. | Actual runtime.status.json starts EF BB BF; old reader uses utf-8. Old authorization checks current matching lease/status without binding the launching session. New v3 wrapper/tests address both. |
| P2 | Old mutex release did not guarantee cleanup of a forcibly terminated wrapper's child builds. | Ordinary Popen child could outlive wrapper. V3 uses process containment and regression tests; old tooling remains historical evidence. |

The five handoff JSON SHA-256 identities matched the stopped board. Their six
commits were applied once, with no conflicts, to a new candidate branch. No
integration-blocking defect was found in the inspected ASSET09/ACT07B/VM05 paths.
Their engineering-only limitations remain: cubic component samples are not
animation playback, package joins are not AI, and condition observation is not
faithful condition truth.

## Gaps against the original document

- M1 remains unmet: observed profile/lookup precedence and original handling of
  documented source exceptions need evidence. Repeating decoded counts cannot
  close those gaps. World owns these tasks; coordinator owns profile capture.
- M2 is not playable traversal. `preview/model.rs` skips skinned meshes;
  `preview/scene.rs` excludes actor model selection; current input is an inspection
  camera. New resource jobs have no preview consumer. Collision decoding explicitly
  reports physics not ready. New presentation and physics lanes own consumers.
- M3 needs original numeric/branch/native/lifecycle measurements and actual effects.
  Reviewed execution paths intentionally refuse Faithful behavior. Scripts has
  runnable capture/trace work and measured execution tasks, not another census loop.
- Native persistence is substantial engineering work but not full world/quest
  persistence. Runtime now has named reference-state and application consumers,
  alongside the residual save validation issue.
- Original UI, voice/audio, quests, combat/AI, first/third-person animation and
  complete base/DLC routes remain explicit future tasks. The eight-lane plan
  assigns their owners instead of declaring them completed by source decoding.
- FO3/TTW/FO4/FO76 and Starfield/crossover remain separately gated. No empty
  future-game crates or per-game agents are needed at this stage.

## Process defects corrected by the setup

The stopped v2 global control was correct, but its coordinator assignment and
several public summaries still said active or named obsolete tasks. V3 initializes
from actual heads and stopped handoffs rather than cloning those task states.

Queues now contain typed ready/blocked state, dependency IDs, actual consumers,
acceptance and independent tasks. Build admission is automatic and nonblocking by
default. Exact initial interface ownership is preauthorized. The old overloaded
world lane relinquishes all preview work to presentation and collision/navigation
to physics. Coordinator integrates development dependencies without waiting for a
public checkpoint and works on shared tooling while frozen proof runs elsewhere.

## Verification boundary

The development base and original-to-integrated mapping are recorded in
preserved-work.json. Its fresh combined validation receipt lives at
`G:\Rust-Fallout-worktrees\integration\local\team-v3-base-review-01\check.json`.
The fresh combined check passed 723 Rust test results, formatting, Clippy across
all targets with warnings denied, and five Python publication checks. Exact receipt
and log SHA-256 identities are in preserved-work.json. Setup-specific validation
is recorded separately in validation.json after those commands finish.

This review/setup does not accept a new gameplay scenario or publish checkpoint46.
Checkpoint45 remains the latest published verification. The older clean proof46
worktree was not built, rewritten or used as fresh evidence. All raw proofs and
draft history remain preserved at their original locations.
