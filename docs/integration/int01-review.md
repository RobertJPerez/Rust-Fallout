# INT-01 candidate review

Reviewer: external integration lane. Approved base:
`63d2173f4a0a3e7fd7c70db105733ab153ddcbe2` on `agents/integration`.
Assignment generation 1 reserves checkpoint 45 for a later complete proof.
This record describes source review and retained evidence audits, not rerun tests
or accepted gameplay.

## Exact dependency chains

Actor candidate order:

1. ACT-02 `7d68691c96fa4a2bbcc93b5fa186f4d8f157c267`, parent
   `b197b97f11d0420207abe17f143bfba50c877eff` (already integrated as `72872b7`).
2. CLAS `37c3e5031cbc38b1d7b8013c874939dabed903f8`, parent ACT-02.
3. CLAS allocation fix `09f3e1e9b660ef64acfdcb2a79d57627f3734322`, parent CLAS.
4. FACT `5b66f9d4dd23d542f618204ba073f0621d1d1bf0`, parent CLAS fix.

ASSET-02 `3bf46d4cba1eb84d41edc7c9a2a416d1aeac7636` has parent
`7d3699beeecc8b195ca936a221cdfe7719b61965`, already integrated as `6cf9708`.
Runtime order is `aea7774c08566e82405137bee5a819e626aa1f36`, parent
`4e7e13adbd805d394d93888526a855c72a5ebdaf`, followed by its child
`474db5460520dec4cf19c1e8a6a2b84bc2660757`.
The actual commit metadata and parents were checked locally.

## Production review dispositions

ACT-02 associations, CLAS with its allocation fix, and FACT are accepted for
candidate integration within their authored source scope. Ordered occurrences,
source namespaces, tombstone winners, complete record/field provenance and raw
numeric bits remain explicit. The CLAS fix checks the winning candidate count
before cloning a key or pushing a candidate. No actor initialization, inheritance,
defaults, faction reaction execution or retail behavior is accepted.

The actor native oracle requires a preflight correction before fresh proof.
`tools/actor-oracle/classes.hpp::project` and `factions.hpp::project` read the
stored body into a vector before the 64 MiB limit, and decompress before checking
the cumulative remaining 256 MiB decoded-byte budget. The inherited scalar
projection in `main.cpp` shares this ordering. Check stored extents and the stored
limit before reading; for compressed records, check the announced decoded size
against both the record and remaining aggregate limits before allocation. Use
authored cases with specific budget diagnostics. Sent to primary for owner routing;
no owner implementation files were edited.

ASSET-02 is accepted for candidate integration within the admitted source branch:
canonical nonempty presence flags, four weights per nonempty vertex, exact source
arrays, and explicit unresolved dependencies. Schema 1 remains the default and
schema 2 is additive. Temporary relation maps in `partition/graph.rs::resolve`
are outside `Catalogue.retained_bytes`; their entries inherit the decoded skin
array budget and scene block ceiling. This is a budget-contract clarification,
not an established unbounded allocation defect. Primary was asked to route the
clarification to the owner. Partition array and repeated relation checks have
explicit bounds, including empty relation work. Pose evaluation, regenerated
triangles/weights and runtime skinning are unverified.

The runtime operand chain is accepted for candidate integration in its read-only
scope. Exact pending events and source windows are bound to the full immutable
source cohort; destinations are distinguished from reads; references use actual
live lists; globals remain unresolved; only an explicitly registered player may
resolve. The follow-up centralizes SCRV declaration support for foreign contexts
and strengthens independent loaded declaration/type/reference tuple auditing.
No VM effects, save-format change or retail readiness is accepted.

## Retained evidence audit

`local/int01-retained-evidence-audit.json` records the completed read-only audit:
26 private seed files matched the seed manifest; three clean committed actor runs
matched their start/finish source snapshots, each committed source file (including
normal checkout line endings), actual frozen binaries, and cold/warm/reordered
native and Rust projections. ASSET-02's 13 source files matched its commit and all
359 evidence files matched the handoff manifest, including frozen binary receipts.
These checks did not execute the owner binaries or rerun their tests.

Observed actor source evidence retains 48,159 association bindings, with 48,158
defined and one deleted voice target. The exact deleted binding is the
DeadMoney actor association to FalloutNV VTYP `029FB1`, whose winning record is
deleted by OldWorldBlues; it must remain a finding in the fresh proof. Retained
CLAS evidence has 100 records and 200 selected fields; FACT has 772 records,
2,513 selected fields and 1,595 bindings. These are observations, not quotas.

Retained ASSET-02 evidence reports 48 schema-1 authored fixtures and 24 altered
reports, 72 schema-2 authored fixtures and 29 altered reports plus four malformed
source cases, and 32 sampled streams with 606 partitions. Fresh proof must retain
specific negative diagnostics and exact binary identities.

Runtime development evidence reports 2,160 events, 2,111 probes, 54,498 selected
native tuples/storage outcomes, 49 source findings, 9,382 unresolved operands
and ten altered-evidence rejections. The local input identity audit is separate
from production verification. Development evidence does not bind a newly built
integration executable; fresh cold/warm captures and an independent native tuple
export are required.

## Private regression preparation

The initial seed omitted five historical evidence inputs required by the existing
full regression. Each was checked against the hash in its immutable published
checkpoint receipt, copied as an independent file into this worktree, and checked
again. `local/integration-historical-inputs.json` records source and private paths,
bytes, digests, and receipt identities. These 298,957,400 bytes remain ignored raw
evidence; no primary proof directory, installation or save was changed.

Next: resolve the actor native preflight finding, reconcile the reviewed candidate
chain and approved source scopes, commit integration tooling, then build and run
the independent frozen candidate proof in a scheduled private resource slot.
Accepted gameplay scenarios remain empty; M1 is still the first unmet milestone.
