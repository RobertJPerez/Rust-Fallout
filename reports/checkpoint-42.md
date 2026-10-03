# Implementation checkpoint 42: shared runtime source ownership

Worlds can own an Arc of the immutable catalogue and outlive the loader or move
to workers. Sharing sources keeps mutable campaign banks independent. Borrowed
callers, snapshot/native-save bytes, cohort checks and recovery policy retain
their contracts. Failed restoration releases temporary owners and preserves state.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 324 Rust tests, Clippy with warnings denied; five publication tests |
| Ownership tests | Eight: lifetime/release, worker transfer, byte equality, separate banks, handles, atomic replacement, recovery and full cohort checks |
| Independent winning source handles | 80,627 |
| Engineering fixture instances / pending events | 2,483 / 2,160 |
| Canonical snapshot | 2,381,563 bytes; exact prior borrowed-state hash |
| Cold / warm cache | Ten plugins in each phase; all cold then all reused |
| Fresh regressions | Full source plans/control/expression/form-list/query/source/runtime/cold-save checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The proof binds 293 source/tooling files at `e0e1a6809d685c2465ef1bd3e432f1214078f49d`,
digest `e2033742e210a5588d955d6d792973b88124f28ff5a4db777f713b75df32f5f3`. Raw evidence remains local:
`local\shared-runtime-42-verified`.

The state fixture uses explicit engineering inputs. Independent source readers
bind the catalogue and schemas; ownership and threading are engine guarantees.
No original running state is captured and no bytecode is executed. Prepared
plans continue to borrow immutable source views rather than becoming an unsafe
self-referential cache. The last world releases the shared source owner.

Execution frames, semantic capabilities, actor/player initialization, scheduling
and retail gameplay remain unfinished. Next: source-bound live event preparation
and bounded runtime components, followed by presentation integration.

See [shared runtime sources](../docs/shared-runtime-sources.md),
[scoped receipt](checkpoint-42-shared-runtime.json),
[verification](checkpoint-42-verification.json) and [remaining gates](../NEXT_STEPS.md).
