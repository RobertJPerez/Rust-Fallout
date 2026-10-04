# Source-root execution admission

`fallout source-plans --execution-admission <request.json>` checks a selected
source graph through existing `PreparedSources` and quest declaration joins.
The request uses schema version 1, the complete `source_cohort_sha256`, and
`roots` containing exact existing definition handles. A stale cohort or handle
is refused before a report can imply that a different winning source was used.

To build a request from the normal `source-plans` report, use
`prepared_source_cohort_sha256` with the caller-selected exact definition handles.
That digest uses the same complete source-receipt and resolved-reference domain
as `PreparedSources`. `winning_definitions_sha256` identifies winning definitions
only. The historical report field `source_cohort_sha256` remains an alias for
that narrower digest for existing comparison tools; its explicit domain is
`legacy_winning_definitions`. It is not an admission-request digest. The request
still names its full digest `source_cohort_sha256`; no check is weakened.

Root priority is canonical `ScriptKey` order; dependencies are traversed breadth
first in physical operand order. A declared SCPT form reference and a resolved
static quest foreign-local declaration can add an edge. These are source
relationships. They do not recover calls, conditional reachability, activation
order or retail scheduling. Placed-reference event lists and dynamic contexts
remain explicit findings through the existing declaration resolver.

The report retains every encountered definition and physical dependency site,
cycle back edges, dependency findings and the first unsupported operation. That
operation includes the exact source handle, instruction/operand SCDA extent,
opcode and encoded calling-reference index when present. Cached source failures
retain an exact failure offset when the existing decoder supplies one; absent
source fields are not assigned a fabricated offset. Graph cycles are inspected
with an iterative traversal; they do not recursively execute or imply a loop
in the game.

Production limits are 32 requested roots, 256 visited definitions, 4,096 edges
and independently 4,096 dependency findings, 65,536 visited instructions and
65,536 cached operand uses across all selected definitions. This last ceiling
counts destination and own-local expression reads even when they produce no
dependency edge. The complete cached use count for each definition is admitted
before its dependency walk; exhaustion identifies the exact definition/version
and first excluded operand's SCDA offset. It returns no partial graph and permits
no execution. Preparation's existing token/node/height limits remain independent;
this pass does not decode or evaluate another expression tree.
Exhaustion returns an error rather than a partial admission. CLI request bytes
are bounded to 1 MiB; prepared-source limits still apply before graph traversal.
The comparison-bundle and admission options are separate requests.

No faithful primitive has a measured original contract yet. Assignment,
conversion, branch, return, native behavior and event lifecycle therefore remain
unsupported. An engineering GetItemCount host read is reported as unverified
native semantics. Even an empty structural event has an unverified lifecycle.
`faithful_execution_admitted`, `retail_lifecycle_verified` and
`retail_parity_accepted` remain false, and the CLI exits nonzero while execution
cannot be admitted. No state, event, save or original installation is mutated.

Authored regressions cover cyclic source and static quest declarations,
unavailable placed-reference context, exact and one-less limits, canonical root
priority, stale handles/cohorts and reuse of cached rejected source. The CLI
regression uses private authored source inputs and metadata-only executable
inspection; it never launches the original game.

The read-only installed-source check used explicit base `FalloutNV.esm`, selected
five winning standalone SCPT editor IDs containing `Mitchell`, and inspected six
definitions with 16 physical dependency sites and five unavailable foreign
contexts. Canonical priority reports the couch trigger's `script_name` header
(`0x1d`) at SCDA offset 0 as the first unverified statement. Source preambles are
included in this diagnostic priority; no assumption makes them an executed no-op.
The source receipt cohort is
`442eeada470823783320cf49794f349f4a71220d4f2018163f1a1ca5e170401a`.
Selection receipts, request, report, private index cache and exact logs stay in
the lane's local evidence directory. This selection does not establish which
event the original game activates first or prove gameplay.
