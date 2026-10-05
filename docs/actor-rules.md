# Actor package context requests

## Explicit canonical equipment lot requests (V3-ACT-16)

`actor_rules::equipment::observe(&World, &foreign::Content, owner, ItemId,
Limits)` selects exactly one current canonical lot through the reviewed bounded
inventory view. Its owned `Selection` retains the complete ordered bank and a
private selected index, exact campaign/cohort/revision/clocks/owner, current count
and optional facts, and the base's winning source kind/flags. It has no mutation
or deserialization authority. `base()` supplies an explicit source-model request
input without choosing a model role or assuming the item is equipped.

Uninitialized inventory and initialized empty inventory return distinct refusals.
Wrong-owner, removed/missing lot and unavailable/deleted source bases refuse.
Two lots with the same base retain separate IDs and exact condition-width bits,
`None`, `Some([])` and populated slot/modification values. Modification keys never
become an ACT08 active modmask. Generic inventory-view item/link/opaque-byte limits
apply before cloning; visit and total projection limits additionally bound the
selection. Observation leaves the authoritative snapshot unchanged.

Persistent `ItemId` is explicitly scoped to the supplied current World campaign.
UI/storage handles use `observe_handle`, which calls the existing canonical
epoch-checked `World::item_id` and rejects restored/foreign World handles before
lookup. `actor-package-context --equipment-item ID --explicit-subject OWNER`
provides a strict cold-snapshot consumer. The package actor root stays explicit;
the equipment source base comes from the selected lot.

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

# Source-backed placed actor spawn inputs

`actor_rules::spawn_state::Requests::prepare` joins one existing canonical
reference through the actor context consumer, resolves its authored ACHR/ACRE
placement to the winning NPC_/CREA base, and combines the existing race/class
initialization-input and template/stat requests. It also publishes every
physical CNTO occurrence with its signed source count, attached COED fields,
binding status and winning item identity when that identity is available.

Direct item definitions are reported as source resolution only. LVLI/LVLC/LVLN
entries retain `leveled_selection_required`; missing, deleted, null and
wrong-kind entries keep explicit reasons. ARMO/ARMA/WEAP kinds are exposed as
possible equipment candidates when their direct winner is resolved. This does
not select an item count, interpret COED, create inventory, choose equipped
state, or evaluate template inheritance and current actor values. The aggregate
keeps actor initialization, inventory initialization and equipment selection
unsupported until their source behavior is admitted. All subrequests remain
campaign/cohort-bound and the complete projection is byte-bounded.

The authored fixture verifies the placement-to-base join, race/class links,
direct armor and leveled-list inventory entries, exact signed counts and COED
retention. It confirms that host state is unchanged and that an undersized item
budget refuses before an observation is published.

# Private engineering actor inventory boot

`actor_rules::inventory_boot::prepare` admits a current source-bound World,
Content, actor catalogue, explicit actor base/owner and `Choice` records. Each
choice names one physical CNTO field index, a positive host count and explicit
canonical Facts. An optional signed source-count claim must match the source
word; source counts remain separate from host counts. Distinct duplicate source
entries stay distinct lots. COED source declarations remain in the full borrowed
inventory definition and never supply automatic condition or ownership facts.
Missing, deleted, null, wrong-kind and leveled item bindings refuse. Direct
selection does not evaluate template inheritance, negative-count semantics,
leveled rolls, respawn, default ammo, equipment or faithful actor initialization.

The private plan pins the complete input canonical snapshot. Its consuming
`apply_private` strictly restores a new World, refuses an initialized bank and
uses the existing initialize/add-item APIs. It returns a native candidate
snapshot and exact source-index-to-ItemId mapping only after every operation and
output budget succeeds. A late failure drops the private world; input/live
state is untouched. The plan has no deserialization or live partial-apply API.
Bounds cover sources, physical fields/index admission, lots, fact links, opaque
bytes, compact input/candidate snapshots and plan/result projections.

`actor-package-context --inventory-boot-request choices.json --explicit-subject ID`
reads bounded explicit choices and emits `actor_inventory_boot.candidate_snapshot`
in the existing canonical snapshot schema. The result is engineering input and
keeps `faithful_initialization_supported` false; the original loaded World is
checked for equality before and after. The candidate can be cold restored by
the existing strict native snapshot consumer.
## Canonical observation of authored race and class inputs

`actor_rules::initialization_inputs::Requests::prepare(&World, &Content,
Manifest, Limits)` consumes the opaque actor source request and seals its campaign
and content cohort. `observe(&World, &Content, Limits)` rechecks both, validates
the existing canonical source header facts and bounds the complete observation.
The optional `actor-package-context --include-initialization-inputs` path uses
its strict current snapshot restore and confirms the before/after snapshots are
identical. It exposes source input completeness, campaign and revision only;
current actor values remain unavailable, actor reference binding is false, and
initialization remains unsupported. No race/class formula, default, state write
or snapshot schema is introduced.

## Canonical item lot to explicit model role

`actor_rules::equipment_render::Requests::prepare(&World, &Content, Choice,
Limits)` admits an explicit owner, current `ItemHandle`, source actor and ACT08
model role. The private request seals campaign, source cohort and canonical
revision. It cannot be deserialized or constructed from an inspector report.
`observe(&World, &Content, &mut RecordStore, &actors::Catalogue, &ArchiveAssets,
Limits)` reacquires the exact ACT16 lot and passes only that selected lot's base
to the existing equipment source producer. Immutable observation getters expose
`selection()` and `model()`; no caller-supplied base is admitted.

Retained intent refuses after cold restore even when campaign, saved IDs, facts
and revision match, because the item handle's World epoch is stale. Another
campaign, removal, transfer or changed canonical revision also refuses before a
joined observation escapes. Prepare a fresh current handle to observe a restored
lot. Content, actor catalogue and fresh store must match the complete canonical
source cohort, including source bytes/hashes and winning headers. Nested ACT16
inventory/selection and ACT08 model budgets remain in force; additional limits
bound source count and the complete joined serialized projection.

`actor-package-context --equipment-item ID --equipment-model-role ROLE` uses
the strict native snapshot restore, explicit subject owner and source actor.
The existing role parser requires caller-supplied armor sex/world/biped choice
or weapon role and mod mask. `None`, `Some(empty)` and supplied slot/modification
metadata remain distinct canonical facts. They do not establish equipped state,
choose a role or derive the mask. Actor origin is not inferred from the owner;
the separate optional actor-context request provides that source join.

The CLI emits `equipment_model` beside the unchanged standalone `equipment_item`
and checks that the authoritative snapshot stays identical. Model requests can
remain missing, ambiguous or unsupported as reported by ACT08. No equip rule,
slot conflict, automatic texture/attachment choice, NIF decoder, live mutation
or save format change is introduced. Equipped state and original behavior remain
unverified.

## Current canonical placement and explicit actor model occurrences

`actor_rules::render_context::observe(&World, &Content, Sources, &View,
&[Occurrence], Limits)` joins the existing ACT15 placement/base observation with
an internally constructed actor `RenderManifest`. `Sources` borrows the existing
placement, actor and dependency catalogues and archive index. The caller supplies
an existing private canonical reference `View`; an actor base is derived solely
from the validated ACHR/ACRE binding. Complete ordered source receipts, winning
cohort and canonical actor/model header facts are checked before output.

The supplied view must match campaign, cohort, revision, authored origin and
current component. For an existing component the existing canonical staging
validator checks its private World epoch and exact state. The private proposal
is immediately dropped; no commit occurs. An unavailable component always gives
an explicit no-mesh outcome, including an equal-state restored unavailable view.
Fresh views can qualify their current saved pose after strict cold restoration;
old views with a component refuse after restoration or state changes.

Each occurrence specifies source FormKey, whole-plugin SHA-256, record file
offset, physical field index/decoded offset/string-frame offset and existing
`RenderRole`. Exact source identity and role must match one existing producer
request. Selection order and repeated choices remain explicit indices into the
single manifest. Unrelated source paths remain diagnostics and do not become
implicit mesh requests. Strict request JSON rejects extra fields, including
inside a role object; the source-role API and raw decoders are unchanged.

The typed outcome distinguishes unavailable component, disabled state,
unavailable scale, unavailable source, empty selection, selected non-mesh paths
and admission. Every selected request is withheld unless the existing canonical
component is explicitly enabled, its scale is available and positive, the
existing render producer admits its source requests and all explicitly selected
paths are model paths. Role collisions, selected HDPT cycles and unavailable or
colliding archives preserve their existing diagnostics. Admission is atomic.

`actor-package-context --include-actor-context --render-path-selection file.json`
uses its strict snapshot and explicit subject, requires the caller's actor root
to agree with the placement join, and emits `actor_render_context`. The result
keeps authored `DATA` and current saved pose/enable separate and confirms that
the authoritative snapshot is unchanged. `admitted_paths()` yields only the
explicit admitted source requests. Output remains in canonical source units;
GPU readiness is false. Transform conversion, NIF/rig/animation selection,
effective equipment and live state mutation remain separate consumers.

Nested context/render/structural limits remain in force. Additional bounds cover
source count, selected occurrences, input identity bytes, logical visits, supplied
view and complete serialized output. Index retention avoids repeated path clones.

The package route consumer joins one explicit physical PACK destination operand
to caller-selected navigation nodes and policy. `route_requests::observe` creates
the reviewed ACT17 destination manifest internally, verifies its whole-source
hash, record and field coordinates against the canonical content cohort, and
loads only explicit cells through the existing source navigation loader. It
builds the published `RouteGraph`; caller reports, meshes and graphs have no
ingress. A repeated or unavailable operand returns a typed unavailable outcome
and no route. A live binding must match the requested destination key.

Endpoints specify both existing node identity and exact cell. A literal CELL
operand must agree with the goal cell. Reference and object operands retain an
explicit, unverified caller mapping to the goal node. Radius/count words remain
source observations. The consumer neither searches for a destination nor chooses
the nearest triangle. Door-bearing links use the caller's explicit pass, reject
or unavailable policy. Local costs, special-link overrides and fallback costs are
finite nonnegative raw `f64` words supplied by the caller. These choices establish
engineering policy; they do not establish retail door or special-link behavior.

`actor-package-context --package-route-request query.json` emits the complete
`actor_package_route` source request and engineering proposal. When an explicit
subject is supplied, its current private view must have an existing component;
canonical staging validates its epoch and exact state, then the proposal is
immediately dropped. This validation never commits state. No caller component
means a refusal. Omitting the optional subject records no actor reference binding.

`actor_rules::package_lifecycle::observe` consumes one explicit physical PKID
occurrence, its exact PACK source, and a caller-supplied route request. It returns
a candidate only for raw PKDT type 6, an admitted literal CELL destination, and a
found source route. POBA, POCA and POEA declarations and embedded script/compiled
source identities retain physical source order and coordinates. That order is not
runtime dispatch order: eligibility, scheduling, event dispatch, embedded script
ownership/execution and actor movement remain unsupported. The result cannot
mutate canonical actor state, and unsupported types or unavailable routes return
errors instead of a successful no-op.
Saved snapshots remain unchanged, and `require_execution()` always refuses
faithful AI even when the engineering path is found.

Source bytes, fields and elements are bounded across all selected cells. Existing
graph and route budgets remain in force. Additional bounds cover cell/source/mesh
counts, strict policy entries, request/view/output bytes and conservative logical
work. The work total reserves the route's complete configured edge, policy,
expansion and path budget before traversal; it is not a count of actual expansions.

### Explicit canonical actor interactions (ACT30–32)

`actor_rules::reference_intent`, `inventory_transfer` and `equipment_intent` accept
an explicit `Claim` containing the compact canonical input snapshot SHA256,
already registered reference ID, expected actor base FormKey and intent
`{"kind":"engineering"}`. `faithful` refuses because original initialization,
pickup and equipment behavior remains unmeasured. The input snapshot is admitted
through existing intrinsic budgets and strict source-bound restoration. ACT15
then freshly qualifies its authored ACHR/ACRE placement and winning actor base.
No reference is registered and no saved inspector view becomes authority.

Reference choices require a live CELL, all six finite position/rotation bit words,
explicit nullable scale bits and explicit enabled value. Positive finite supplied
scale and signed zero are preserved by the canonical Pose/State constructors.
Missing scale remains null. Source DATA and disabled flags do not supply defaults.
The private candidate stages and commits the selected component exactly once.

Transfer choices select one ItemId belonging to the qualified actor and an
already registered destination with an explicitly initialized inventory. ACT16
admits the exact lot and winning base. The existing whole-lot transfer preserves
its ID, count and every fact; same-base lots never merge. Transfer to the same
owner follows the canonical no-op contract and preserves the revision.

Equipment choices require `supplied_slots`, whose null, empty array and ordered
unique u16 array remain distinct. Omission refuses. Only that selected lot's
equipped_slots changes through the existing facts validator and replacement.
Condition width and raw bits, ownership, ammo, modification order, quest/script
links and opaque bytes remain exact. Equal supplied facts still advance the
canonical revision. There is no slot sorting, retail mapping, auto-unequip or
conflict inference.

The optional `actor-package-context --actor-reference-intent request.json`,
`--actor-inventory-transfer request.json` and `--actor-equipment-intent request.json`
flags emit immutable private candidate reports. Each supplied request independently
uses the same original input snapshot. An explicit chain requires saving a
candidate snapshot and making a new digest-qualified request against it. Source,
request, snapshot, inventory, fact/slot/work and complete output budgets apply;
Borrowed snapshot bytes are admitted before canonical source-name validation;
the exact candidate length delta is admitted before cloning its snapshot, and
late output refusal drops the private result. The original world and input files
remain unchanged. Strict wire types reject duplicate or unknown fields.

The connected proof checks component → slot metadata → exact-lot transfer → cold
restore, with nonempty unrelated script/event state and full snapshot conservation.
The independent interactions oracle reuses existing source/placement/lot readers
and derives complete expected candidates from caller input alone. Engineering
proofs do not establish original gameplay parity.

### Selected actor contexts and directed faction inputs (ACT34/33)

`context_batch::observe_batch` accepts an ordered explicit list of existing
canonical reference IDs. It admits the whole source cohort once, rejects repeated
IDs and joins every ACHR/ACRE placement through the existing ACT15 checks before
publishing output. Base declarations remain borrowed and appear once per exact
canonical FormKey and bound winner, sorted by key. Per-reference placement and
current state remain separate; absent or disabled components supply no defaults.
An empty selection is valid. A nonactor, missing or inconsistent late join refuses
the whole batch. The legacy single-context output and refusal order are retained.

Aggregate bounds cover references, unique bases, placement plus unique base
fields, join visits, current-view bytes and the full serialized projection. Every
borrowed current row is charged before source joins, and the complete projection
is counted before any canonical View clone. Structural usage counters count source
receipt comparisons, selected rows, joined physical fields and seven fixed checks
per placement. The 64-placement fixture retains one base and admits its source
cohort once; these counters express the defined work charge, without timing claims.

`faction_pair::observe_pair` derives both actor bases from fresh ACT15 contexts,
then reuses ACT07 Requests to validate their physical SNAM/FACT declarations.
It indexes the target actor's declared faction keys and emits only directed XNAM
matches. Source occurrence, relationship field and target occurrence indices
retain physical order and multiplicity. Signed ranks and modifiers and raw
reaction words remain exact. Reverse requests are computed independently. Template
ambiguity, unresolved memberships/relations and unsupported current membership
remain explicit unavailable inputs. Same-faction defaults, symmetry, aggregation,
disposition, reputation and effective reactions remain unavailable.

Both endpoint occurrence/FACT extents and current bytes are admitted before their
projection allocations. Pair multiplicity and visit charges are checked before
allocating expanded rows; a streaming count checks the complete output first.
The pair visit charge includes source/context preparation, borrowed preflight,
target indexing, the full directed match count and all three projection/build
passes. These serialize-only observations accept no retained Requests or handles
and provide no mutation authority. Fresh calls after cold restore rejoin the
canonical state; old canonical views remain invalid for staging.

`actor-package-context --actor-context-batch selected.json` reads a bounded array
of nonzero reference IDs. `--actor-faction-pair pair.json` requires explicit
`from_reference`, `to_reference`, `expected_from_actor` and `expected_to_actor`;
both FormKey claims must match the fresh placement joins. Unknown pair fields,
incorrect claimed bases and late source failures refuse without emitting a report.
Omitted flags preserve legacy inspector output. The independent decision-input
oracle reuses the protected Reader, native declarations and existing context
oracle, verifying physical SNAM/XNAM bytes and complete schema4 expectations.

Focused validation on 2026-10-04 passed format, the eleven actor runtime targets
(67 passed, 0 failed, 2 ignored), and warnings-denied Clippy for `fallout-runtime`
and `fallout-cli`. The native actor oracle built and passed all six allocation-order
checks. Thirteen base/override batch and directed-pair CLI reports matched the
independent raw-source oracle, including mixed and 64-reference selections,
permuted input order, forward/reverse/same endpoints and template references.
Wrong-base, wrong-cohort and late-invalid batch/pair requests each refused without
creating a report. The legacy default CLI report matched the preserved ACT15 output
byte-for-byte (2,406 bytes; SHA-256
`2753ac00a2e7c45f34f258cfd7d7057a4e0035bdfd838c09c9021a1a2448f7e8`). The proof
summary is `local/v4-evidence/actors-01-team-v4-20261004-20261004T222612Z-0f8c3e22/proof-summary.json`
(SHA-256 `0e40f33ccd2e5e74304e9c8d5eb782db67a0792bb94ffa12d7622e2b0d5f74ce`).
These are authored-fixture/source-reader results only: retail parity remains
unaccepted, current faction membership and reaction evaluation remain unsupported,
and gameplay scenarios accepted remains zero.
