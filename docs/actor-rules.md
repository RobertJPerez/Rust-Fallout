# Actor package context requests

## Canonical actor reference source context (V3-ACT-15)

`actor_rules::context::observe(&World, &foreign::Content,
&actors::placements::Catalogue, &actors::Catalogue, ReferenceId, Limits)` returns
a read-only observation of one existing canonical reference, its exact authored
ACHR/ACRE placement and uniquely bound winning NPC_/CREA base. It acquires a fresh
canonical `reference_state::View` and checks ordered source receipts, winning
content identity, placement kind/flags and base binding provenance. The complete
source placement and actor scalar fields retain physical offsets and hashes.

The authored transform/scale and raw disabled flags remain source declarations.
The optional current canonical pose/enable remains a separate reference view;
missing state stays unavailable. This request does not register a reference,
initialize an actor, reset pose, infer enable or select a template value.
Unregistered/dynamic, missing/deleted/non-actor origins and unavailable/wrong-kind
bases refuse. Changed source bodies/cohorts cannot supply this join. Selected
fields, source rows, visits and complete JSON projection have finite limits.

`actor-package-context --include-actor-context --explicit-subject ID` adds the
observation after strict current snapshot restoration. Its resolved base must
match `--actor-root`; a mismatch refuses instead of binding another actor.
The host verifies the canonical snapshot remains identical. Default package
context output remains unchanged.

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

## Faction and relationship source requests

`actor_rules::factions::Requests::prepare` joins one selected actor's physical
SNAM associations to the existing winning FACT catalogue. It requires the same
ordered source receipts and winning-content identity as the canonical world.
Each available target must match the original association's record kind, plugin,
offset and flags. Repeated SNAM entries remain separate requests, including their
signed eight-bit ranks and three unused bytes. Null, missing, deleted and
wrong-kind bindings remain visible without a fabricated faction source.
The unique actor ACBS scalar field also retains the authored disposition base
and exact field origin; it does not become an evaluated relationship value.

Every selected FACT retains its complete physical field origins, hashes and
findings. Ordered XNAM requests point to those exact fields, preserving signed
modifiers, raw reactions and FACT/RACE target bindings. Rank declarations, unused
float bits, opaque labels and legacy absent flag bytes remain source data.
Relationships are not recursively traversed or combined into hostility.

The pinned xEdit Common definitions at revision
`9fb016884bec138ea6c7b872cec831537d464c3e`, lines 8801–8833, establish these
SNAM/XNAM layouts and target domains; FNV lines 4721–4754 establish FACT fields.
The FNV template callback's faction category mask `0x04` produces an explicit
unsupported selection issue. Missing or repeated actor ACBS is also explicit.
No effective inheritance, membership, rank, reaction or condition truth is
evaluated, including when a source mask is clear.

`Requests::observe` checks the original campaign/catalogue, existing `Content`
cohort and any caller-supplied canonical reference. It reports that reference's
origin without treating it as the selected actor's base form. Absent context is
permitted for a source request; unavailable references are rejected. The adapter
does not mutate world state and supports a same-campaign canonical restore.
Limits count repeated faction fields and relationship requests, physical visits,
occurrences and compact projection bytes before admitting the result.

The existing headless command adds this projection with
`--include-faction-requests`. Its package output is unchanged by default. Both
requests use the same restored canonical state; this flag does not add a faction
state store or change snapshot schemas. Retail faction initialization and
relationship behavior remain unverified.

Validation on 2026-10-04 passed five faction request tests, five package request
tests, ten shared condition tests and seven existing faction decoder tests,
formatting, warnings-denied runtime/CLI Clippy and the CLI build. The frozen
headless projection matches both the complete canonical host observation and
the independent native raw-plugin reader: seven SNAM occurrences, 25 repeated
FACT fields and 15 ordered relationship requests. Signed rank/disposition and
modifier extremes, raw reaction values and absent legacy bytes remain exact.
Missing references/actors and missing required snapshot arguments reject; a
modified relationship fails independent comparison. Default package output is
unchanged, and all plugin, snapshot, descriptor and frozen proof input hashes
match before and after. No gameplay acceptance follows from these checks.

## Authored scalar initialization requests (V3-ACT-09)

`actor_rules::stats::Requests::prepare` joins the existing actor scalar catalogue
and actor dependency catalogue to one selected actor and the canonical `World`.
It requires exact ordered source receipts and winning-content identity. The
existing bounded `template_manifest` supplies physical ACBS/TPLT origins, category
declarations, structural closure, candidate sources, cycles and source issues.
Every candidate's retained scalar definition must match that exact source/header
and decoded body hash. Candidate scalars remain declarations, including when
template inheritance is unavailable; the adapter never selects a parent's value.

The root requests preserve each physical ACBS, DATA and NPC DNAM field, its index,
offset, hash, raw scalar values and ambiguity. Missing fields remain explicit.
Components identify their pinned category: fatigue/level/calculation bounds/speed
and stat fields use Stats `0x02`, barter gold uses AI Data `0x10`, and
karma/disposition/creature type use Traits `0x01`. A selectable root declaration
requires a unique field and the corresponding clear template flag. This source
availability does not authorize initializing an actor or using a current value.

The pinned xEdit revision `9fb016884bec138ea6c7b872cec831537d464c3e` distinguishes
NPC ACBS flag `0x10` (auto-calc stats) from CREA flag `0x10` (Swims). NPC attributes
and DNAM retain that automatic-calculation declaration, or an unavailable
configuration if ACBS is missing/repeated. NPC base health keeps its separate
authored declaration. No creature flag becomes an NPC auto-calc request. Complete
NPC/CREA definitions and the Stats/auto-calc callbacks establish these source
categories; editor visibility and fixups do not establish engine formulas.

`Requests::observe` requires the same campaign/content identity and reports the
current canonical revision. It admits narrowed limits for template sources,
links, closure, visits and issues, scalar fields/decoded bytes, component requests
and serialized projection bytes. Every evaluated component and the current actor
values remain null; actor reference binding, initialization, automatic calculation
and original behavior verification remain false. The adapter has no state mutation
or snapshot-schema extension and works over a cold-restored same-campaign world.

The existing `actor-package-context` command adds these requests with
`--include-stat-requests`, using its required strict source-bound native snapshot
and actor root. The existing package output is preserved when the flag is absent.
This is a reachable initialization-input consumer; faithful initialization still
requires original measurements and the canonical actor-reference/state boundary.

Validation on 2026-10-04 passed six stat request tests, ten existing faction/package
request tests, an explicitly enabled installed source/cold-state test, format,
runtime/CLI warnings-denied Clippy and the CLI build. Seven complete authored CLI
and cold-host observations match the independent native scalar and physical
template readers. They cover signed/raw extremes, auto-calc versus Swims, inherited
categories, missing/repeated fields and absent current values. Exact/one-less
limits, winning overrides, template cycles, changed cohorts and campaigns refuse
as expected. An altered scalar fails independent comparison; required snapshot,
missing actor/reference and default-output checks pass.

The installed actor `FalloutNV.esm:104C0C` also matches complete independent
requests through the frozen CLI and a cold canonical restore: two candidate
sources and thirteen component requests. Its selected source bodies and template
origins were freshly read and checked against the preserved independent raw
reader baseline under matching source receipts. All eleven original plugin and
descriptor inputs, and all nine private snapshot/tool/baseline inputs, keep their
before/after hashes. Proofs and earlier harness failures are preserved in
`local/v3-act09-stats-20261004-01`; these checks accept no gameplay behavior.

## Explicit package execution capabilities (V3-ACT-13)

`actor_rules::packages::Requests::capability` accepts canonical `World`/`Content`,
an optional explicit subject, an operation and narrowed capability limits. It
reuses `observe` with faithful intent, preserving each shared adapter refusal
and exact physical CTDA site. An engineering item-count observation never becomes
condition truth or an execution grant.

The report adds the existing complete actor fields and PACK scalar definitions,
dependency findings, physical event fields and embedded loaded-script handles,
versions, declarations, references and issues. These objects borrow the existing
catalogues; no new condition, package or script decoder runs. Repeated PKID
occurrences keep repeated inputs and consume repeated field/entry/visit budgets.
Unavailable targets retain their original binding and have no package inputs.

`physical_source_complete` only means that each declared package's physical body
and prepared condition source are retained. It can be true for repeated/opaque
fields, zero CTDA or an empty package list. Source findings and template requests
remain explicit. That property grants no eligibility, schedule or AI behavior.

`Capability::require_execution` always returns a typed refusal. Eligibility
requires original condition truth and grouping rules. Selection additionally
requires effective package-selection rules. Scheduling also requires measured
schedule-word interpretation and package execution. All three operations refuse
for empty/source-complete inputs. No actor instance, event or mutable AI state is
created. Canonical campaign/cohort/content/reference checks precede the result;
fields, event fields, scripts, entries, visits and compact projection are bounded.

```powershell
.\target\debug\fallout.exe actor-package-context --install LOCAL_INSTALL --load-order ORDER_JSON --actor-root FalloutNV.esm:104C0C --native-snapshot PRIVATE_SNAPSHOT --package-capability scheduling --output local/package-capability.json
```

This explicit operation request emits the detailed refusal and exits 1. Accepted
choice strings are `eligibility`, `selection` and `scheduling`. Without this
option the existing context report and exit behavior are unchanged. If an
engineering observation is also requested, it remains separate from the
capability's faithful refusals. Snapshot input and world state remain unchanged.
The independent `package_capabilities.py` checks the complete request using the
existing native scalar/association reader, the raw PACK dependency reader and
the canonical snapshot's bookkeeping; no observed field supplies expected data.

Validation on 2026-10-04 passed eight package tests (three new capability groups)
and ten shared condition tests, format, data/runtime/CLI all-target Clippy and
the CLI build. The three operations in four authored cases match complete
independent source and canonical snapshot projections through the frozen CLI
and direct restored host. Empty lists, zero conditions, unavailable targets,
repeated declarations, event-kind findings and absent/compiled embedded units
remain exact. Exact/one-less limits and campaign/content/reference mismatches
refuse. Default context equals the preceding faithful observation; engineering
counts of five stay separate from an identical faithful execution refusal.
Changed execution admission or a missing refusal dependency rejects comparison.

All three installed Doc Mitchell (`FalloutNV.esm:104C0C`) capabilities also match
the full independent reader and refuse execution. They retain 230 fields,
81 event fields, 27 embedded script declarations and 419 visits. All eleven
original plugin/descriptor inputs and eight private tool/snapshot/order inputs
keep their hashes. Evidence and earlier harness/oracle failures remain under
`local/v3-act13-capability-20261004-01`; no gameplay behavior is accepted.
