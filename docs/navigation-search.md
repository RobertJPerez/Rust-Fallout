# Cooperative source route search

`navigation::search` adds `SearchLimits`, `StepBudget`, `SearchProgress` and a
private-constructed `RouteJob`. `RouteGraph::start_search(start, goal, limits,
&policy)` borrows the exact graph and an immutable `Fn` policy. `advance(step)`
returns pending or complete progress with step and cumulative work; no partial
Route is exposed. `finish(self)` returns only a completed Route. Cancellation,
drop or incomplete finish releases private state and both borrows. Caller-owned
graph and policy state remain with their owners. No continuation is serialized.

The original one-shot `RouteGraph::route` keeps its signature, `FnMut` policy,
RouteLimits defaults, strict-less improvement, heap cost/node tie order, goal
expansion charge, errors and diagnostic ordering. It drains the same search
core with checked graph-derived storage reservations. No second route solver or
source parser is introduced. Existing route JSON remains unchanged.

Distance and predecessor arrays, the heap and active node/edge cursor survive
each yield. Each edge invokes the policy once. Stale pops count independently
of expansions. Path-node copies and the two result-vector reversals also yield;
completion includes reconstruction. Error during work poisons the job, so it
cannot later expose a partial result. A rejected step ceiling before work leaves
the untouched job available for a valid step.

Each advance bounds heap pops, expansions, edges, path nodes, copy reservations
and reversal swaps independently. A copy reservation admits one edge decision
and possible diagnostic, or one path-node/preceding-link copy. Source identities
are capped at 4,096 UTF-8 bytes each and reasons at 1,024 bytes, bounding each
indivisible copy. The caller supplies a bounded, stable policy; this API bounds
its invocation count and retained response, not arbitrary caller code execution
or wall time. The CLI uses the existing finite policy with at most 256 special
cost entries, source flags and door eligibility.

Before allocating job buffers, admission scans names and actual graph counts
and reserves fixed arrays, heap, path/result capacity, both diagnostic buffers
and worst-case identity/reason strings. Nonnegative costs and strict-less
relaxation settle each node once; at most E+1 candidates are pushed. Heap pops
are globally limited, including stale entries. Global copy/reversal allowances
derive from E and path capacity. Buffer capacity is checked before a path copy
can exceed it. Admission includes array initialization and is an explicitly
bounded synchronous operation; graph/source construction remains separately
bounded. Cleanup is bounded by the admitted collections and has no hard timing
promise.

Cooperative ceilings are the current RouteLimits ceilings, 300,001 heap pops,
64 MiB private job storage, 600,003 admission visits and 4,096 identity bytes.
Callers may lower these. All-zero step budgets return pending without work; a
host can later provide a useful budget. The CLI requires positive step fields
and bounds total advances to 1,000,000, preventing an unbounded host loop.

`fallout navigation-search --install INSTALL --load-order ORDER --request JSON`
is the real consumer. Its strict request supplies explicit CELLs, the existing
route policy, resource limits, step budget, maximum advances, trace limit up to
64 and optional `cancel_after`. It reuses the protected RecordStore, exact
source CELL/NAVM loader, graph admission and policy. It stores bounded initial
trace samples plus final progress, preserving one frontier. Cancellation reports
`search_state: cancelled` and `route: null`; advance exhaustion or any late
refusal emits no successful report.

Source metadata, the conservative graph reservation, fixed overhead, bounded
trace and the actual job reservation share one source identity budget. That
remaining allowance restricts the job before its allocation. The complete
pretty report and newline are counted against 64 MiB before emit. The request
is limited to 1 MiB. Source-qualified IDs, raw portals, missing neighbors and
special-link/door decisions keep their existing meaning.

Literal fixtures cover cycles, zero costs, equal alternatives, stale heap
entries, missing/special neighbors, cancellation, one-under limits and paused
reconstruction. Small and large slices preserve exact source order and policy
invocations. This is an engineering proposal; no actor movement, collision,
save-state update, original scheduler or gameplay readiness is implied.
