# Implementation checkpoint 43: pending-event source preparation

The runtime prepares an exact pending journal sequence against its live instance,
retained context and immutable winning source. Authored BEGIN offset and event ID
select the window together, including repeated IDs. Preparation borrows the world
and preserves its state; it does not execute bytecode or acknowledge events.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 332 Rust tests, Clippy with warnings denied; five publication tests |
| Runtime tests | Eight: identity/context, repeated IDs, queue lookup, findings, exact budgets, restoration and wrapped deque |
| Pending events checked | 2,160 |
| Prepared frames / instruction headers | 2,111 / 50,415 |
| Retained source findings | 49 control-structure findings, independently correlated |
| Attempted source bytes / successful full-plan operand uses | 1,168,191 / 85,708 |
| Canonical snapshot | Exact separately persisted engineering-state hash, unchanged |
| Fresh regressions | Full source plans, shared ownership, control/expression/source tables, queries, runtime and cold saves |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The proof binds 297 source/tooling files at `4e7e13adbd805d394d93888526a855c72a5ebdaf`,
digest `c03b9cc378d7ec62fb265ccaba3c2963978678962dff388cd6dbffacd3ec6aca`. Raw evidence remains local:
`local\event-frames-43-verified`.

Every frame joins the separately persisted journal's sequence, context and live
definition. Source windows match their winning compiled regions and independent
native header digests. Aggregate counters are derived again from source reports.
Full source preparation precedes window selection; failures cannot produce a
partial frame. Inspector budgets also charge failed attempts' source bytes.

The fixture supplies explicit engineering state. Source reader comparisons prove
source facts; immutable preparation and unchanged state are engine guarantees.
No original running event list, lifecycle or execution is accepted. The inspector
returns exit code 1 after writing its diagnostic report because 49 findings remain.

M1 remains the first unmet master-brief milestone. Live operand resolution,
numeric/coercion behavior, native/branch effects, continuations, actor/player
initialization, scheduling and gameplay scenarios remain open. Two isolated
workers are advancing actor source associations and skin/animation prerequisites
while the primary agent continues runtime work and integrated verification.

See [event preparation](../docs/live-event-preparation.md),
[scoped receipt](checkpoint-43-event-frames.json),
[verification](checkpoint-43-verification.json) and [remaining gates](../NEXT_STEPS.md).
