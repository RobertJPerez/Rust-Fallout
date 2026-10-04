# Actor package context requests

`actor_rules::packages::Requests::prepare` joins one winning actor's existing
ordered PKID associations to `actors::package_dependencies`. It requires the
canonical world's exact ordered source receipts and winning-content identity.
Repeated packages keep separate occurrences and physical field origins. Null,
missing, deleted and wrong-kind targets remain visible with their original
bindings. A defined PACK must match the original link's winning header and
source offsets before its condition requests are prepared.

Every physical CTDA site comes from the existing privately prepared condition
record. Requests use `execution::condition::Request`; there is no new condition
or embedded-script decoder. Embedded event scripts are retained by the existing
source catalogue and are not executed here. Conditions do not acquire invented
AND/OR groups or thresholds, and packages do not acquire eligibility or schedules.

`Requests::observe` accepts an explicit canonical reference context and the
existing condition intent. The context's authored origin is reported; the
adapter does not infer a placed actor or claim that this reference belongs to
the chosen base actor. Missing references fail before dispatch. A caller may
leave the context absent, which produces the shared adapter's precise missing
subject outcome. Requests reject a different campaign or catalogue and work
after a same-campaign canonical restore. Observations disclose the current state
revision, so later reads can be distinguished from earlier state.

The default faithful intent refuses unverified retail semantics. Explicit
engineering observations can read the shared GetItemCount host query against
already initialized canonical inventory. They preserve source operands and the
query contribution trace; condition truth, effective package eligibility,
scheduling and original numeric return remain unavailable. Unknown functions
and selectors keep the shared adapter's unsupported outcomes.

The existing inventory ACBS supplies its exact flags and source field origin.
Missing/duplicate configuration remains ambiguous. The xEdit FNV schema at
revision `9fb016884bec138ea6c7b872cec831537d464c3e`, lines 2280-2297,
classifies template AI packages with mask `0x20`; it justifies an explicit
unsupported template-selection issue, not retail inheritance. Even without
that mask, this consumer reports authored declarations rather than an effective
AI package stack.

Limits admit physical package occurrences, repeated condition counts and field
visits before allocation. They also charge the shared adapter's site-search
work before preparing requests. Query contributions share one aggregate budget,
and compact observation serialization has a separate byte bound. Queries are
read-only and do not mutate the canonical world or snapshot.

The headless consumer restores a bounded native runtime snapshot:

```powershell
.\target\debug\fallout.exe actor-package-context --install LOCAL_INSTALL --load-order ORDER_JSON --actor-root ORIGIN_PLUGIN:LOCAL_HEX --native-snapshot SNAPSHOT_JSON --explicit-subject REFERENCE_ID --engineering-observation --output local/package-observations.json
```

Without `--engineering-observation` it uses the faithful intent. An optional
`--condition-executable` can supply the exact pinned original descriptor image
for an authored plugin fixture; the existing fingerprinted executable reader
still checks it. The command reports the descriptor and snapshot receipts and
restores through the canonical `Snapshot::decode` / `World::restore` APIs.
No default actor state, native repository, save format or original installation
is modified.

Validation on 2026-10-04 passed five actor package tests and ten shared condition
tests, formatting and warnings-denied Clippy for runtime/CLI targets. A frozen
CLI cold-restored the authored snapshot and matched the complete expected
observation: seven physical PKID occurrences, five CTDA requests and four query
contributions. Repeated packages remain repeated; null, missing, deleted and
wrong-kind targets remain unavailable. Canonical source/plugin/snapshot and
pinned descriptor executable hashes were unchanged before and after the proof.

The actual headless consumer also demonstrated faithful refusal, missing-subject
outcomes, rejection of a missing canonical reference and actor, changed source
snapshot refusal, and required snapshot argument validation before input reads.
Tests cover same-campaign restore, another campaign, changed content, missing or
duplicate configuration, template flags and exact aggregate work/projection
limits. These are engineering host observations; retail package selection,
condition truth and scheduling remain unverified.
