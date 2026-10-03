# Overnight work, October 3, 2026

The autonomous run advanced New Vegas through implementation checkpoints 15–37.
The goal started at 05:04:47 UTC; the release handoff was checked at
17:19:43 UTC, more than twelve hours later. The original games
have not been recreated yet. M1 remains the next unmet acceptance gate.

| Checkpoints | Implemented and checked |
| --- | --- |
| 15–19 | Compiled-script framing, executable command/event metadata, authored script tables, expression envelopes and native operands |
| 20–25 | Operand associations, original condition layouts, quest/dialogue ownership, winning topic membership, immutable loaded scripts and static quest attachments |
| 26–27 | Independent compressed-record extraction and typed winning condition dependencies |
| 28–30 | Canonical script instances/event contexts, native snapshots with explicit recovery, and current live-owner foreign locals |
| 31–32 | Source-bound base inventories, authored leveled lists and bounded dependency graphs |
| 33–36 | Canonical item banks, atomic quantity changes, explicit native save migration, source-kind validation and shared item-count query traces |
| 37 | Initial census publication that preserves current verified metadata |

The final workspace passes formatting, 283 Rust tests, Clippy with warnings denied
and five Python publication tests. Checkpoint 37 freshly reruns the source/runtime
regression chain, including independent comparisons of all 640 command descriptors
and 80,467 winning condition bindings. All 464 installation files, totaling
9,907,238,722 bytes, still match the baseline.

The handoff audit checked all 23 checkpoint receipts against their source revisions,
verified that the three current aliases match checkpoint 37, and checked the current
263 source/tooling files against the committed snapshot. Earlier receipts keep their
original revisions and limits.

The release launcher initially lacked its renderer. Both release executables are
now built together. The launcher configuration check and one offscreen Goodsprings
startup passed, producing a 1280×900 textured capture with 1,089 source vertices and
2,048 triangles. Captures and original asset reports remain local. This is a startup
check, not the complete GPU suite, a desktop input test or a retail comparison.

To open the inspection view from this checkout:

```powershell
.\target\release\fallout-playtest.exe
```

The next work remains effective retail profile verification, the documented vanilla
format exceptions and remaining content semantics, followed by VM execution,
actors/AI/combat, complete campaign persistence and measured retail comparisons.
The shared count query currently evaluates our canonical host item state; original
argument coercion, form-list behavior and retail numeric return behavior are open.
Other Fallout games remain in scope after the New Vegas dependency gates.

See [checkpoint 37](checkpoint-37.md), [release handoff metadata](overnight-2026-10-03-release-handoff.json),
[remaining gates](../NEXT_STEPS.md) and [inspection instructions](../docs/terrain-textured-preview.md).
