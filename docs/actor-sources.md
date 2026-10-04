# Immutable actor source fields

## Explicit creature model-list directory requests (V3-ACT-14)

`dependencies::Catalogue::creature_parts_manifest(root, &explicit_mesh_directory,
assets, CreaturePartsLimits)` adds physical CREA NIFZ archive requests to the
existing render manifest. The caller supplies a normalized `AssetPath` beneath
`meshes/`; the request never infers a directory from MODL or actor identity.
Every original frame retains its raw bytes and physical offset through
`manifest_path_index`. Default structural and render projections remain exact.

The concrete installed diagnostic is GSCheyenne, FalloutNV.esm:10588D (version15,
record offset11972408): MODL declares `Creatures\Dog\Skeleton.nif`, while NIFZ
contains `dogskin.nif`, `eyessetblue.nif` and an empty physical frame. With the
explicit engineering directory `meshes/creatures/dog`, nonempty safe names get
archive candidates. Empty frames retain `empty_source_path`; there is no claim
that an empty frame is a retail array terminator. Absolute, traversal, drive,
unsafe and overly long names stay visible with exact lookup refusal.

Missing/repeated ACBS, Model/Animation template mask64 and repeated physical
NIFZ fields retain all frames without selecting source candidates. Missing
models, empty lists, collisions and any existing render issue prevent complete
part-request admission. Candidate counts and bytes aggregate the original
structural manifest and added directory requests under the same limits; lookup
bytes, visits, requests and total JSON projection have separate finite budgets.
Admission concerns physical requests under an explicit choice. Effective part
assembly and rig playback remain unsupported.

`actor-sources --include-dependencies --dependency-root FalloutNV.esm:10588D
--creature-model-directory meshes/creatures/dog` emits `actor_creature_parts`.
The option requires exactly one creature root. The independent Python oracle
uses the existing raw plugin/string-frame reader and archive metadata reader;
it compares the entire new projection without another NIF or asset decoder.

## Template category declaration requests (V3-ACT-03)

`actors::dependencies::Catalogue::template_manifest(root, TemplateLimits)` uses
the existing inventory/list/template graph and its bounded structural closure.
It adds the missing chosen-root category requests with exact inventory ACBS and
TPLT field indices, offsets, hashes, raw flags and original bindings. Singleton
defined actor templates expose structural candidate sources with their actual
winning headers and plugin/body hashes. They are not inherited field winners.
Inventory actors remain outside this candidate prefix, although the existing
broad structural closure continues to retain them.

The ten category masks come from pinned xEdit FNV callbacks, lines 2165-2370,
and Common `wbTemplateFlags`, lines 8429-8453. A clear category mask identifies
the chosen actor's authored source declaration. A set mask remains an unsupported
inheritance request; it never chooses a parent field or substitutes editor
defaults. The declared source index refers to the chosen root in canonical key
order, including when an ancestor has a smaller key. Every request states that
runtime values have not been evaluated.

Repeated TPLT, null/missing/deleted/wrong-kind links, missing or repeated ACBS,
unknown template bits and missing required template links retain explicit
diagnostics. A legal LVLN/LVLC template remains a leveled selection request;
its list is available in the existing structural graph and no random candidate
is chosen. Direct actor template cycles use the existing shared graph helper
and refer to exact candidate source indices. The candidate prefix visits each
actor once and charges fields, graph joins and link/source/issue limits. All
explicit CLI roots share aggregate template budgets.

The existing `actor-sources` command accepts `--include-template-dependencies`
with `--include-dependencies` and explicit `--dependency-root` values. Its
optional `actor_template_dependencies` projection can be compared completely
with the independent original-byte reader's matching flag. Template and render
requests can be inspected together; their scopes stay explicit. Effective
inheritance, leveled selection, auto-calculated statistics and gameplay remain
unverified.

Validation on 2026-10-04 passed all 21 dependency tests (one unrelated installed
test remained ignored), formatting and warnings-denied data/CLI Clippy. The
authored template projection independently matched two candidate sources and
all ten requests. The installed DocMitchell projection independently matched
the complete scalar/dependency report, two candidate sources, one TPLT origin
and ten category requests with no template issues; all root declaration indices
correctly identify source index 1. The independent authored combined render and
template projection also matched. An altered category mask failed comparison,
and missing flags failed before input reads. Deliberately incomplete authored
records kept their existing source-finding exit after successful comparison.

## Selected actor render declarations (V3-ACT-01)

`actors::dependencies::Catalogue::render_manifest(root, &ArchiveAssets,
RenderLimits)` adds a consumer adapter over the existing `Manifest`. Requests
point to that manifest's physical path indices; selected edges point to its
original link indices. Source headers, plugin/body hashes and winning-content
identity accompany the selected actor, race and explicit head-part/hair/eye
records. Existing `manifest()` and default inspector output retain their schema.

One unambiguous existing inventory ACBS occurrence supplies raw flags and
template flags, with its exact field origin. The adapter classifies direct NPC
sex and matching NAM0/NAM1 race declarations using the pinned FNV schema. It
retains head/body part indices and model versus texture roles. It follows the
actor's explicit head-part/hair/eye links and HDPT extra links; catalogue-wide
race hair/eye options, inventory actors and template candidates remain outside
the selected requests. Repeated links stay in the structural manifest. Its
existing cycle diagnostics remain available and the bounded selected walk
visits each source once.

The source evidence is xEdit `9fb016884bec138ea6c7b872cec831537d464c3e`,
FNV lines 2165–2182 (traits mask `0x01`), 2299–2316 (model/animation mask `0x40`),
6798–6955 (NPC ACBS/links), 7340–7469 (race contexts/head indices), and Common
7502–7513 (body indices). These editor category masks justify conservative
refusal; their visibility callbacks do not prove retail inheritance. Model
template requests remain unsupported, while a traits-template flag prevents
selecting a race/sex from the child. Missing or repeated ACBS never acquires a
default configuration. Unknown part indices and unavailable/ambiguous links
retain precise manifest references. Repeated model/part declarations remain
ambiguous, including duplicate empty list fields.

Archive lookups reuse the manifest's exact missing/single/collision/unsafe
outcomes. NIFZ/KFFZ relative bases remain unresolved. A source MODL is labelled
an actor model; the adapter does not infer skeleton structure from a filename.
Equipped state, FaceGen composition, template inheritance, animation playback and
retail lookup precedence remain separate capabilities. Presentation and assets
can consume these source requests without introducing another decoder.

The reachable headless consumer is:

```powershell
.\target\debug\fallout.exe actor-sources --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --include-dependencies --include-render-dependencies --dependency-root ORIGIN_PLUGIN:LOCAL_HEX --output local/render-actor.json
```

The new flag requires explicit roots before opening inputs. All roots share
manifest budgets and a separate aggregate render budget for selected sources,
requests, issues and visits. The existing independent raw-source oracle accepts
`--include-render-dependencies` with `--root` and independently obtains ACBS
configuration from its retained original plugin handles. Runtime state and save
formats are unchanged. This is an engineering source consumer, with no new
gameplay acceptance.

Validation on 2026-10-04: all nine actor regression suites passed (72 tests;
one separately invoked installed test). The installed test confirmed
DocMitchell's exact EDID at `falloutnv.esm:104C0C` through the retained winning
body. His six selected sources produce 26 requests with no selection issues;
all 26 have one physical archive candidate. The CLI's complete source and
render projection matched the independent original-byte reader, including all
6,455 actor scalar definitions and 6,626 dependency definitions. This verifies
source declarations and candidate metadata, not visible composition or gameplay.

The authored fixture independently matched six sources and seven requests;
tests also cover missing/colliding archives, repeated/empty fields, template
refusal, changed winning head parts and exact admission budgets. An altered
render source hash was rejected by the CLI comparison. Missing required flags
failed before opening inputs, and the existing dependency-only projection still
matched its independent reader. Formatting and warnings-denied Clippy passed
for affected data/CLI targets. Deliberately incomplete authored actor records
retain the inspector's existing nonzero source-finding exit after a successful
comparison; those reports do not represent accepted retail behavior.

ACT-01 adds `actors::Catalogue::load(&inventory::Catalogue, actors::Limits)`.
It decodes the remaining authored NPC_/CREA scalar fields over the existing
inventory production loader. The actor catalogue borrows the exact inventory
definitions, source receipts, retained records and winning metadata digest. It
does not rebuild CNTO/COED/TPLT bindings or join unrelated inputs by record count.
The inventory definition remains available through `inventory_definition()`;
raw unknown and unselected fields remain available through `record()`.

This is source decoding. No actor is initialized, no template is inherited and
no auto-calculated statistic, level conversion, combat rule or AI behavior is
implemented or accepted. Source hashes and a winning header digest together bind
the cohort; a header digest alone does not prove equality of record payloads.

## Admitted disk layouts

| Field | Authored values | Shape |
| --- | --- | --- |
| NPC_/CREA ACBS | Fatigue, barter gold, level word, PC-level-mult flag, calculation bounds, speed multiplier, karma float bits and signed disposition | 24 bytes |
| NPC_ DATA | Signed 32-bit base health, seven attribute bytes, unchanged unused trailing bytes | At least 11 bytes; the pinned schema explicitly declares a variable trailing byte array |
| NPC_ DNAM | Fourteen skill values and fourteen skill-offset bytes, both unsigned | 28 bytes |
| CREA DATA | Type, combat/magic/stealth bytes, signed 16-bit health/damage, two unused bytes and seven attributes | 17 bytes |

The attribute order is Strength, Perception, Endurance, Charisma, Intelligence,
Agility, Luck. Skill order is Barter, Big Guns, Energy Weapons, Explosives,
Lockpick, Medicine, Melee Weapons, Repair, Science, Guns, Sneak, Speech, Survival,
Unarmed. These are disk labels and ordering, not effective runtime actor values.
ACBS flags and template flags are already retained by the inventory catalogue;
the actor projection does not replace those existing fields. The level word is
never divided, clamped or used to generate a runtime level.

NPC record versions 14 and 15 and creature versions 9, 11, 13, 14 and 15 were
independently observed in this installation and are explicitly admitted. Other
versions return an unsupported error, even when lengths happen to match. This
does not imply a format change between every other version: those variants have
not been verified here. Every current NPC DATA field is 11 bytes. Authored tests
exercise the schema-declared unused tail, including a 25-byte field, without
inventing a legacy-version threshold or claiming retail behavior for that tail.

Every physical field retains its kind, decoded offset, extent, SHA-256 and order.
Duplicate selected fields remain separate occurrences with findings. Missing ACBS
and DATA produce findings without fabricated defaults. DNAM absence remains
absence; the pinned schema does not mark it required. Creature DNAM and other
unselected fields remain opaque. Integer signs and all karma bit patterns,
including NaNs, are retained exactly. Record, byte and field budgets bound the
decoder. Truncations, unsupported known lengths, tainted input and direct decoding
of deleted records are rejected with source context.

Deleted inventory winners remain tombstones, with empty scalar fields and no
fallback to older definitions. Their `record_version` is explicitly null because
the inventory catalogue intentionally retains no decoded record for tombstones.
Their original header remains bound through the winning metadata digest, source
plugin hash, file offset and flags; no version is inferred from a predecessor.

## Reference pins

The disk reference is [xEdit at
9fb016884bec138ea6c7b872cec831537d464c3e](https://github.com/TES5Edit/TES5Edit/tree/9fb016884bec138ea6c7b872cec831537d464c3e).
MPL-2.0 notices were inspected in the existing project audit. This slice consults
layout facts and authors its own Rust/C++ code; it imports no Pascal implementation
and does not execute the Delphi application.

| Read-only reference | SHA-256 | Selected lines |
| --- | --- | --- |
| Core/wbDefinitionsFNV.pas | `89b1420415b858e004429e89a828348a1df71f5c8b199d4ea66e02f0ca41b012` | CREA 4276–4423; NPC_ 6798–6955; NPC editor fixup 2478–2493 |
| Core/wbDefinitionsCommon.pas | `e616b6546f6df74d88ec98bb870906db380c985963d011e4931c5726682b7d26` | Level union decider 5704–5716; editor level-mult clamp 923–936 |

The schema's editor level-mult clamp and NPC NAM5 fixup are not disk values and
are never applied. The primary agent owns any corresponding sources.lock.json
update after integration.

## Verification

The private ACT-01 development comparison used the ten plugins in
`profiles/nv-inspection-order.json`. Its source receipts and complete winning
metadata matched the separate C++ reader. That reader opens the original plugins
itself, resolves masters and winners, and decompresses selected original payloads
using the unchanged authored `oracle-common` reader. It never consumes Rust-decoded
records. C++ is confined to this offline comparison tool.

| Compared source facts | Observed result |
| --- | --- |
| NPC_/CREA winners | 4,220 / 2,235; 6,455 total |
| Complete physical actor fields | 204,929 equal, including opaque hashes and offsets |
| Selected scalar occurrences | 17,130 equal |
| Decoded actor body bytes | 5,337,465 |
| Selected scalar duplicates or missing required fields | Zero in this cohort |
| Index-cache phases | Ten cold misses; ten warm hits; ten hits after swapping the independent final two content packs |
| Authored direct-reader comparison | Five winners, including one tombstone, nine physical fields, eight scalars and five retained findings; cold/warm equality |
| Deliberately altered oracle scalar | Rust comparison rejects a changed fatigue word |
| Unsupported NPC version 16 / 18-byte CREA DATA | Both Rust and the offline reader reject each case |

Raw worker evidence stays local:
`G:\Rust-Fallout-worktrees\actors\local\act01-scalars-20261003-03`,
`local\act01-authored-comparison-20261003`, and
`local\act01-authored-inputs-20261003\negative-results-02`.
The private worker-comparison files pin binary/report hashes and actual command
arguments. They are not integrated checkpoint receipts. Earlier failed wrapper
runs remain in separate local directories and are not counted as completed runs.

Eight focused Rust tests pass. Formatting and all-target Clippy for fallout-data
and fallout-cli pass with warnings denied. Tests cover exact signed/float/byte
storage, legacy tails, field occurrences, missing values, version dispatch,
truncation, extended lengths, budgets, source lifetime, namespaces, overrides,
tombstones, actual cold/warm cache reuse and unrelated plugin reordering.

Run the inspector from the worker worktree after building its private target:

```powershell
$env:CARGO_TARGET_DIR = 'G:\Rust-Fallout-worktrees\actors\target'
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked -p fallout-data --test actors
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 clippy --locked -p fallout-data -p fallout-cli --all-targets -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 build --locked -p fallout-cli --bin fallout
.\target\debug\fallout.exe actor-sources --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --output local/actors-new.json
```

Build the native reader with `tools/actor-oracle/build.ps1 -BuildDirectory` pointing
to a private local directory. `tools/actor-oracle/compare.ps1` accepts explicit
`-Fallout`, `-Oracle`, `-Install`, `-LoadOrder` and a new `-RunDirectory`. It creates
its own index cache and compares cold, warm and reordered projections through the
Rust inspector's `--compare-oracle` option. Reordering requires independent final
plugins; use `-SkipReordered` for a dependent fixture. `-AllowSourceFindings` is
only for authored fixtures expected to retain findings and return diagnostic exit
1. The original cohort returns exit 0 in every phase. This tooling writes no
installation files and does not claim original-game behavior or gameplay parity.

## Ordered actor associations (ACT-02)

`actors::associations::Catalogue::load(&mut RecordStore, &actors::Catalogue,
Limits)` joins the admitted actor source to a store only after verifying a unique
normalized-plugin map of exact source lengths/hashes and the exact winning header
digest. Duplicate normalized receipts are rejected. Each owner is then located
using its stable FormKey in the supplied store, with plugin identity, record
offset, flags, kind and retained header checked. No prior store index is reused.
Unrelated store reordering can succeed; changed override winners or even a
same-length payload change with identical header counts must fail the join.

The separate association definitions point to physical field indices in the
scalar definition. They preserve authored order and every occurrence. The
original field metadata and bytes remain in the joined source catalogue.

| Authored link | Admitted source shape | Target schema |
| --- | --- | --- |
| SNAM faction | Form word, signed rank byte, three unused bytes; 8 bytes | FACT |
| NPC_ RNAM race / CNAM class | One form word each; 4 bytes | RACE / CLAS |
| VTCK voice | One form word; 4 bytes | VTYP |
| INAM death item | One form word; 4 bytes | LVLI |
| SPLO actor effect | One form word; 4 bytes | SPEL |
| EITM unarmed effect | One form word; 4 bytes | ENCH or SPEL |
| PKID package | One form word; 4 bytes | PACK |

Creature RNAM is attack reach and CNAM is an impact dataset. This slice does not
reinterpret them as NPC race/class links. Faction, actor-effect and package lists
retain repetitions and order without deduplication. Repeated singleton links are
findings. Required NPC voice/race/class field absence produces findings without
defaults. Target null, missing, deleted and defined states use the existing
inventory binding model; target-kind agreement is a separate optional fact.
Missing/deleted targets and wrong kinds are reported without source repair.
No faction state, effect execution, package selection or template inheritance is
initialized by resolving a declaration.

Additional pinned reads: `wbDefinitionsCommon.pas` lines 8801–8812 define the
signed SNAM rank and padding; lines 6531–6543 confirm that FNV retains the three
unused bytes. The FNV association declarations are within the previously listed
CREA/NPC schema blocks, plus SPLO at lines 4061–4062. The existing reference file
hashes and xEdit revision above remain unchanged.

The private complete direct-reader comparison agrees in cold/warm/reordered runs
on all 48,159 ordered bindings: 17,008 factions, 4,220 races, 4,220 classes, 5,762
voices, 2,296 death items, 2,374 actor effects, 395 unarmed effects and 11,884
packages. Of these, 48,158 targets are defined and one voice target is deleted.
That authored voice link on `deadmoney.esm:00AE30`, at decoded offset 183, remains
an `association_target_deleted` finding. Each original report is written with
exact independent equality, then returns diagnostic exit 1. The original game's
handling of that link remains unmeasured.

Seven association tests pass in addition to the eight scalar tests. They cover
ordered authored bindings, signed ranks and unused bytes, target domains,
null/missing/deleted/wrong-kind targets, missing and duplicate fields, creature
field overloads, bounds, tombstones, source-content changes, changed override
winners and stable ownership across reordered stores. Direct authored-reader
comparisons and deliberately altered rank/invalid SNAM tests supplement those
checks. Formatting and fallout-data/CLI all-target Clippy pass.

Use `actor-sources --include-associations` to add the association projection.
Pass `-IncludeAssociations -AllowSourceFindings` to the comparison wrapper for
the original cohort's retained tombstone finding. Scalar-only inspection and
comparison remain available. ACT-02 raw development evidence is under
`local\act02-associations-20261003-01`,
`local\act02-authored-comparison-20261003-01`, and
`local\act02-authored-inputs-20261003\negative-results`.

The comparison wrapper now captures HEAD, dirty status, a hash/length manifest
of every tracked and nonignored untracked file, and both executable hashes before
comparison. It verifies those inputs at completion before publishing its private
worker receipt. An injected source-file change was rejected and produced no
completed receipt; its raw attempted run is retained at
`local\act02-source-freeze-rejection-20261003`. Successful runs include source-start
and source-finish manifests. These are worker evidence, with root responsible
for fresh integrated checkpoint publication. The earlier ACT-01 receipt predates
this runner correction; its exact binaries are preserved at
`local\act01-frozen-tools` with their original hashes.

Placed-actor extras, required race source inputs and bounded actor
dependency closure remain next source dependencies. Runtime inheritance,
initialization and original gameplay measurements stay with the primary lane.

## Authored class inputs (ACT-05-CLAS)

`actors::classes::Catalogue::load(&mut RecordStore, Limits)` owns an immutable
catalogue of winning CLAS definitions. It retains the complete winning header,
source plugin/hash/offset/flags, ordered physical field metadata and decoded body
for each nondeleted winner. Tombstones retain their winning header without
reading a deleted payload. Full source receipts and the exact winning header
digest are exposed for future joins; record counts or store indices alone cannot
establish shared source identity.

The pinned FNV schema at `wbDefinitionsFNV.pas` lines 4146–4169 defines the
selected layouts. The reference revision and file hash above remain unchanged.
An independent direct-file investigation found 101 physical definitions and 100
winning classes in the original ten-plugin cohort. The winning nondeleted header
versions are 14 (29 records) and 15 (71 records). Both have the same selected
fixed layouts. Other nondeleted header versions are explicitly unsupported.

| Field | Selected exact layout |
| --- | --- |
| DATA, 28 bytes | Four signed 32-bit tag-skill words, unsigned 32-bit flags and services, signed training-skill byte, unsigned maximum-training-level byte, two unused bytes |
| ATTR, 7 bytes | Seven authored attribute bytes in schema order |

Signed words, unknown flag/service bits and unused bytes are retained without
enum normalization, clamps or editor defaults. Missing DATA/ATTR and repeated
selected fields produce findings. Repetitions remain ordered; no value is chosen
as an effective class. Invalid selected lengths fail explicitly. Other physical
fields remain opaque, with exact offsets, lengths, hashes and retained bytes.
This slice does not apply tagged skills, train actors, initialize services or
compute effective attributes.

Five focused tests pass, covering signed extremes, flags, unused bytes, training
bytes, attributes, ordered duplicates, extended opaque fields, missing inputs,
unsupported versions, truncation/extents, decoding budgets, compressed bodies,
master-relative overrides, unread tombstones and cold/warm/reordered ownership.
The native direct-source projection builds in the private ACT-05 class directory.
`actor-sources --include-classes` and comparison-wrapper `-IncludeClasses` add the
class projection while preserving the scalar-only and association modes.
Formatting and fallout-data/CLI all-target Clippy pass with warnings denied.
All twenty scalar, association and class tests pass together.

The independent original comparison agrees in cold/warm/reordered phases on all
100 classes, 499 physical fields, 200 selected scalar occurrences and 9,628 decoded
body bytes. There are no class findings. The combined scalar/association/class
reports retain ACT-02's one deleted voice target and return diagnostic exit 1.
Ten plugin cache misses become ten warm hits and ten reordered hits. The authored
direct-reader comparison agrees on six winners including a tombstone, twelve
physical fields, eight selected occurrences and four retained findings in all
three phases. A deliberately changed tag-skill word is rejected; malformed
27-byte DATA, 8-byte ATTR and nondeleted version 16 fail in both readers. Scalar
default mode continues to compare successfully without admitting class sources.

Raw development comparisons, investigation and authored/negative files stay
under `local\act05-class-comparison-20261003-01`,
`local\act05-class-authored-comparison-20261003-01`,
`local\act05-class-investigation-20261003`, and
`local\act05-class-authored-inputs-20261003\negative-results`. Both complete
comparison runs captured and verified unchanged source manifests and binary
hashes. These are source comparisons; root owns fresh integrated publication and
no actor class behavior has been accepted against the original game.

## Authored faction inputs (ACT-05-FACT)

`actors::factions::Catalogue::load(&mut RecordStore, Limits)` owns exact winning
FACT source inputs, retaining headers, provenance, decoded nondeleted bodies and
all physical fields. Tombstones are kept without reading their payloads. It
exposes full source receipts and the winning header digest for future joins.
Bindings are resolved in the current owning record's namespace using the existing
inventory binding model. No earlier store indices are retained or reused.

Pinned FNV lines 4735–4755 define FACT, lines 4721–4733 define rank declarations,
and Common lines 8814–8832 define relations. FNV lines 2738–2764 remove CNAM during
an editor after-load operation. This reader preserves those original CNAM bytes.
Both reference file hashes and the xEdit revision above remain unchanged.

| Field | Admitted source facts |
| --- | --- |
| DATA, 1 or 4 bytes | Raw flags byte; the four-byte layout additionally contains a second flags byte and two unused bytes. Missing optional bytes remain absent. |
| CNAM, 4 bytes | Exact unused float bits, including non-finite bit patterns |
| RNAM, 4 bytes | Ordered signed rank-number declaration, without grouping title fields |
| XNAM, 12 bytes | Authored FACT/RACE target, signed modifier and raw group-combat-reaction word |
| WMI1, 4 bytes | Authored reputation target with REPU domain |

The direct original investigation found 772 physical definitions and 772 winners,
with 3,923 physical fields and no tombstones. DATA has 34 one-byte occurrences and
738 four-byte occurrences; XNAM has 1,547, RNAM has 112, CNAM has 34 and WMI1 has 48.
Observed admitted header versions are 1, 2, 3, 4, 8, 9, 10, 11, 13, 14 and 15.
Legacy DATA1/CNAM4 occurs in versions 1/2/3/4/8; DATA4 occurs in the later observed
versions. Layout absence is read directly, without inventing a version migration
or filling absent bytes. Other record versions and DATA widths, including the
unobserved two-byte partial layout, remain explicitly unsupported in this slice.

Relations and rank declarations retain repetitions and order. Repeated DATA,
CNAM or WMI1 singleton declarations and absent DATA produce findings. Target null,
missing, deleted and defined states are preserved; target-kind agreement is a
separate optional fact. Missing/deleted and wrong-kind targets produce findings.
Male/female titles, insignia and other fields remain opaque with exact retained
bytes and metadata. No editor normalization, effective rank, crime/reputation
state, faction hostility calculation or combat reaction is executed.

Seven focused faction tests and all-target fallout-data/CLI Clippy pass with
warnings denied. Tests cover legacy absence and unused bits, signed ranks and
relation modifiers, raw reactions, target states/domains, duplicate/missing
declarations, version/length/framing admission, budgets before candidate clones,
valid extended opaque fields, compression, namespaces, tombstones and cache/order
reuse. All 27 scalar, association, class and faction tests pass together.

The direct original comparison agrees in cold/warm/reordered phases on all 772
factions, 3,923 fields, 2,513 selected occurrences, 1,595 bindings and 76,397
decoded bytes. All target winners are defined and no faction findings occur.
The scalar/association/class projections also remain exact, including the class
candidate-budget fix and the known voice tombstone finding. The authored reader
agrees on six winners including a tombstone, 23 fields, 19 selected occurrences,
10 bindings and seven findings in all three phases. DATA3 and unadmitted DATA2,
XNAM11, RNAM5 and nondeleted version 16 fail in both readers. A changed reaction
word is rejected, while scalar default mode continues to compare successfully.
Cache phases have 0/10/10 hits for the original cohort and 0/3/3 for authored input.

Raw comparisons and negative results remain under
`local\act05-faction-comparison-20261003-01`,
`local\act05-faction-authored-comparison-20261003-01`, and
`local\act05-faction-authored-inputs-20261003\negative-results`.
Investigation/version-layout evidence is under
`local\act05-faction-investigation-20261003`. All completed comparison runs verify
their actual source/HEAD/dirty-state manifests and executable hashes unchanged.
The separate class budget fix checks the limit before each candidate key clone;
its fresh authored comparison is at
`local\act05-class-budget-followup-comparison-20261003`. That run truthfully
captures the then-uncommitted faction work in its source manifest. Original class
equality is additionally covered by the combined faction comparison.
These worker source comparisons do not establish original faction behavior or
replace root's integrated checkpoint verification.

## Placed actor core and first extras (ACT-03)

`actors::placements::Catalogue::load(&mut RecordStore, Limits)` owns winning
ACHR/ACRE source headers, raw parent metadata, bodies and ordered physical fields.
It reuses the unchanged `world::decode_placement` for NAME, DATA, XSCL and the
existing optional core fields. The retained typed placement is available through
`Definition::placement()`. The independent JSON core projection obtains exact
position, rotation and scale words by calling `f32::to_bits` on those already
decoded values. There is no separate Rust transform parser or coordinate
conversion. Unknown source fields retain their bytes and metadata.

The first bounded extra slice selects XEZN4 (ECZN encounter-zone binding), XMRC4
(REFR merchant-container binding), and XLCM4 (signed level modifier). Base bindings
use the existing parsed NAME word and exact NPC_/CREA schema domain for ACHR/ACRE.
All bindings use the current owning store location. Null, missing, deleted and
wrong-kind facts remain explicit, with findings for the latter three states.
Duplicate extra declarations are retained and reported. Optional absence stays
absent. Winning parent IDs remain raw and relative to the winning source plugin,
including when an override moves the actor into a different cell.

Pinned FNV lines 3113–3165 and 3203–3247 contain the selected layouts and domains,
within the ACHR/ACRE declarations. Reference revision/file hash remain unchanged.
The direct investigation found 7,761 physical definitions and 7,681 winners:
3,942 ACHR15, 3,736 ACRE15, one ACRE11 and two ACRE9. These exact kind/version
combinations are admitted. There are no winning tombstones in this cohort.
All have one NAME4 and DATA24; 79 have XSCL4. Core transforms are finite and scales
positive, matching the established world decoder's admission policy. The selected
extras have 16 XEZN4, 86 XMRC4 and 305 XLCM4 occurrences, with no duplicates.

Limits precede candidate key clones, physical field allocations, world decoder
core-map allocation, binding creation and strict body decoding. Deleted winners
retain headers/parent metadata without payload reads or earlier-definition
fallback. Six focused tests pass, covering source-bit preservation, signed zero
and subnormal scale, exact core getters, signed level modifiers, target states and
domains, ordered duplicates, absent extras, extended opaque fields, kind/version
dispatch, existing core rejection policies, budgets, compression, winning parent
moves, unread tombstones, namespaces and cold/warm/reordered ownership.

Use `actor-sources --include-placements` and comparison-wrapper
`-IncludePlacements` to add this source projection. Original and authored full
direct-reader comparisons agree in cold/warm/reordered phases. The original
cohort has 7,681 winners, 22,665 physical fields, 407 selected extra occurrences,
7,783 defined bindings and 681,344 decoded bytes, with no placement findings.
The other scalar/association/class/faction projections remain exact. The combined
reports retain the known voice finding and return diagnostic exit 1. Authored
comparisons agree on five winners including a tombstone, 22 fields, nine extras,
ten bindings and five findings, retaining the moved winning cell parent.
Original cache phases have 0/10/10 hits; authored phases have 0/3/3.

Both readers reject invalid XLCM3/XEZN5, missing NAME, non-finite core transforms
and unadmitted ACHR14. Deliberately changed core bits and winning parent metadata
are rejected; the scalar-only default still compares successfully. All 33 actor
tests, formatting and fallout-data/CLI all-target Clippy pass. Completed worker
runs verify their actual source/head/dirty manifests and executable hashes
unchanged. Source comparison does not initialize actors or accept retail behavior.

Raw comparisons are at `local\act03-placement-comparison-20261003-01` and
`local\act03-placement-authored-comparison-20261003-01`; negatives are under
`local\act03-placement-authored-inputs-20261003\negative-results`.
Investigation and fixtures stay in `local\act03-placement-investigation-20261003` and
`local\act03-placement-authored-inputs-20261003`. The pre-implementation framing
check at `local\act03-placement-fixture-framing-20261003` checked the established
header/scalar path only, and is not placed-actor semantic evidence.
Health/count/patrol extras remain separate future source slices. This
catalogue does not infer a cell/world encounter-zone fallback, effective level,
merchant inventory, live placement or actor initialization.

## Native body allocation bounds (ACT-08)

Review identified that the offline native scalar/class/faction/placement readers
checked some body budgets after allocating or inflating the body. Their shared
owned `tools/actor-oracle/body.hpp` now computes the smaller of 64 MiB per record
and the catalogue's remaining 256 MiB decoded budget before reading source bytes.
Stored lengths are checked before the locked Source reader is called. Compressed
declared lengths are checked before constructing the inflater's output, and the
remaining limit is also passed to the unchanged authored zlib decoder. Complete
decoded length and Adler checks still apply, and the ledger advances only for a
successfully validated body. Shared/pinned oracle infrastructure is unchanged.

Six native checks instrument the reader/decoder callbacks to verify that
oversized stored bodies are never read and over-budget declared payloads never
reach decompression, including a partially consumed cumulative budget. Exact
remaining uncompressed and an independently authored valid compressed case pass.
The build wrapper builds/runs these checks in the caller's private native build
directory. Four direct-file NPC_/CLAS/FACT/ACHR fixtures with hostile inflated
prefixes are rejected by both readers, and the native diagnostic confirms the
pre-inflate rejection point. Inputs/results are retained under
`local\act08-body-prefix-negatives-20261003`. Full original comparisons with the
corrected binary agree in cold, warm and reordered phases for every requested
scalar, association, class, faction and placement projection. Counts remain
6,455 actors, 100 classes, 772 factions and 7,681 placements; the known deleted
voice source finding remains explicit. Authored class, faction and placement
comparisons also agree in all three phases, including malformed/duplicate source
findings, tombstones and current-store bindings. Each completed run confirms
that its actual source snapshot and executable hashes stayed unchanged.

Evidence is in `local\act08-body-bounds-comparison-20261003-01`,
`local\act08-class-authored-comparison-20261003`,
`local\act08-faction-authored-comparison-20261003` and
`local\act08-placement-authored-comparison-20261003`. This tooling fix does not
alter the Rust catalogues or establish gameplay parity.

## Placed linked references (ACT-03 second slice)

Pinned xEdit FNV lines 3157 and 3239 declare ACHR/ACRE `XLKR` as a four-byte
linked reference with seven allowed target kinds: REFR, ACRE, ACHR, PGRE, PMIS,
PBEA and PLYR. The existing placement catalogue now retains each occurrence as
`Value::LinkedReference`, using unchanged inventory binding in the winning
source's master/self namespace. Physical order, offsets, body/field hashes,
target provenance and null/missing/deleted/wrong-kind facts remain explicit.
Repeated singleton occurrences produce
`multiple_placed_linked_reference_fields`; none is selected or discarded.
Absence remains absence. There is no link traversal or activation/AI inference,
and declared cycles are retained as ordinary source bindings.

The existing kind/version admission, unchanged world placement decoder, source
receipts and record/field/body/binding bounds remain in force. The inspector uses
the existing `--include-placements` flag. No other catalogue, runtime, save,
dependency, store, inventory or shared-file contract changes are required.
The independent native reader adds its own field/domain projection while
retaining the ACT-08 preallocation guard. Original byte investigation observed
1,361 ACHR and 491 ACRE XLKR4 fields without duplicates. Full original comparisons
agree in cold, warm and reordered phases across every requested projection:
7,681 placements, 22,665 fields, 2,259 selected extras, 9,635 bindings and
681,344 decoded bytes with zero placement findings. All 1,852 linked targets are
defined: 1,372 REFR, 438 ACHR and 42 ACRE. The association section still retains
the previously measured deleted voice finding; no fallback clears it.

Focused tests cover all seven allowed domains, self-links, null/missing/wrong
kind/deleted targets, ordered duplicate findings, exact binding-limit admission,
XLKR3/5 rejection, master/self namespaces and cycles across cold/warm/reordered
header caches. Authored comparison inputs include all seven domains and are at
`local\act03-linked-authored-inputs-20261003-02`.
The native authored comparison agrees in all phases on eight winners including a
tombstone, 29 fields, 15 XLKR occurrences, 22 bindings and 12 findings. Its targets
include all seven declared kinds plus an explicit wrong-kind PACK, null, missing
and deleted references. Both readers reject XLKR3/5. Deliberately altered raw
binding words and target file offsets fail the complete placement comparison.
All 35 actor tests (including eight placement tests), formatting,
fallout-data/CLI all-target Clippy and the
private native build's six allocation-order checks pass.

Comparisons are at `local\act03-linked-comparison-20261003-01` and
`local\act03-linked-authored-comparison-20261003-01`; negative results are under
`local\act03-linked-authored-inputs-20261003-02\negative-results`. Completed runs
verify actual source/head/dirty manifests and executable hashes unchanged. These
are worker source checks; they do not accept gameplay or constitute an integrated
checkpoint.

## Remaining source investigation

Read-only byte investigation for the next RACE slice is retained at
`local\act05-race-investigation-20261003`. The supplied original order has 31
physical and winning RACE definitions, all version 15, without deleted winners:
22 from FalloutNV, six from HonestHearts and one each from DeadMoney,
LonesomeRoad and OldWorldBlues. Their 3,518 physical fields occupy 144,223 decoded
bytes. DATA36, PNAM4 and UNAM4 occur once in every winner (93 selected fields).
Pinned FNV lines 7369-7397 define seven ordered signed skill/boost byte pairs,
two unused bytes, four height/weight float words and flags in DATA. Lines
7420-7421 define the main/face clamp floats. Source bits do not establish applied
race statistics or FaceGen behavior. These measurements bound the separately
approved scalar catalogue below; they do not constitute its comparison receipt.

Independent package investigation is retained at
`local\act06-package-investigation-20261003`. It found 4,888 physical PACK records
and 4,885 winners across versions 1/2/3/9/10/11/13/14/15, with 88,491 fields and
1,455,709 decoded bytes. The observed PKDT layouts are 4,872 twelve-byte fields
and 13 eight-byte fields; all winners have one PSDT8. PLD2 has 472 twelve-byte
occurrences across 236 winners, so repeated occurrences must be preserved rather
than silently collapsed. There are 3,777 CTDA28 and 24 CTDA20 fields. These facts
prepare an independently bounded package task; this investigation does not decode
package unions, migrate editor defaults, group conditions or execute AI/scripts.

## Race scalar sources (ACT-05 first RACE slice)

`actors::races::Catalogue::load(&mut RecordStore, Limits)` owns winning RACE
headers, source identity, decoded bodies, ordered field metadata and findings,
with complete source receipts and the exact winning-content digest. The first
scope admits observed version 15 and DATA36/PNAM4/UNAM4 only. Other fields retain
opaque hashes/offsets and full body bytes; no model, voice, age, relation or hair
bindings are decoded yet. Deleted winners retain header/provenance without
reading their body or applying a version/default migration.

DATA retains seven ordered `SkillBoost { skill: i8, boost: i8 }` pairs, two unused
bytes, male/female height and weight raw words, and every flag bit. PNAM/UNAM
retain main/face clamp words, including exact signed zero, subnormal and
non-finite bit patterns. Fields do not apply clamps or infer effective race or
FaceGen state. All three pinned required declarations produce missing-field
findings when absent; duplicate singletons retain each physical occurrence and
its finding. No default is manufactured to satisfy an absent declaration.

Limits precede matching candidate clones, body reads/inflation and field vector
growth. Per-record stored/declared/decoded bodies use the smaller of 64 MiB and
the remaining 256 MiB catalogue budget. The independent native projection uses
the ACT-08 body guard and directly reads the original fields. The additive
`--include-races` flag emits `actor_races` and compares its complete projection;
the private inspector options struct preserves the existing optional sections.
Minimal CLI main flag/dispatch/finding wiring is declared for integration.

All 40 actor tests (including five RACE tests), formatting and fallout-data/CLI
all-target Clippy pass. The private native build passes its six allocation-order
checks. Original comparisons agree in cold, warm and reordered phases on every
requested source section, including 31 RACE winners, 3,518 fields, 93 scalar
fields and 144,223 decoded bytes with zero race findings. The known association
voice finding remains explicit. The inspector options refactor retains all
existing complete projections.

Authored comparisons agree in all phases on seven winners including an unread
tombstone, 18 fields, 16 scalar fields, 396 bytes and six source findings. They
retain exact non-finite/zero/subnormal words, signed skill order, unused bytes,
duplicates/absence, extended opaque fields and master/self winner identities.
Both readers reject DATA35/37, PNAM3, UNAM5, versions14/16 and a hostile declared
compressed prefix. Changed height bits and winning-header trailing bytes fail
the complete race comparison; the scalar-only default still compares equally.

Evidence is at `local\act05-race-comparison-20261003-01`,
`local\act05-race-authored-comparison-20261003-01` and
`local\act05-race-authored-inputs-20261003\negative-results`. Completed runs verify
their actual source/head/dirty manifests and executable hashes unchanged. This
is an immutable source slice; race application, actor/player initialization and
gameplay remain separate.

## Package scalar sources (ACT-06 first PACK slice)

The team-v2 coordinator approved `ACT-06-PACK-source-contract` on October 3,
2026. `actors::packages::Catalogue::load(&mut RecordStore, Limits)` owns each
winning PACK header, source receipt, body and ordered physical fields. The
optional `actor-sources --include-packages` inspector section and independent
direct-source reader expose `actor_packages`. Minimal CLI flag, dispatch and
finding wiring in `main.rs` is declared for integration.

| Field | Exact source layout |
| --- | --- |
| PKDT8 | General flags u32 at 0, raw type u8 at 4, unused byte at 5, behavior flags u16 at 6 |
| PKDT12 | PKDT8 prefix plus type-specific flags u16 at 8 and two unused bytes at 10 |
| PSDT8 | Signed i8 month/weekday/date/hour at 0/1/2/3, unsigned duration u32 at 4 |

All multibyte words are little-endian. PKDT8 has an explicitly absent tail;
unknown raw types, flags and negative schedule bytes remain intact. No masks,
enum coercion, editor defaults, date conversion or calendar rules are applied.
The pinned xEdit FNV declaration is lines 6990-7074 and 7124-7135, with deciders
at 2813-2826; its revision and file hash remain those listed above. Width selection
follows physical size independently of the admitted record version.

Observed PACK versions 1/2/3/9/10/11/13/14/15 are admitted. Other nondeleted
versions and other PKDT/PSDT widths fail with source context. Tombstones retain
their winning header without reading or migrating their bodies. Candidate,
stored/inflated-body, cumulative decoded-byte and field limits apply before the
corresponding allocation. Every selected physical occurrence remains ordered;
missing required fields and each later duplicate receive explicit findings.
Other fields, including CTDA and embedded scripts, remain opaque and body-retained;
later slices must use the scripts lane's decoders.

The comparison wrapper optionally checks team authorization and all current-run
STOP mailboxes while polling its own native/Rust child processes. A cancellation
stops those owned children and leaves the attempt incomplete. Standalone use
does not require team files. Earlier completed evidence and frozen tools remain
unchanged. A private guard test verifies current-run cancellation of the launched
child, rejection of a changed lease, and preservation of old-run STOP rows.

All 46 actor tests, including six PACK tests, pass. Formatting and affected-package
all-target Clippy with warnings denied pass. The private native build passes six
allocation-order checks. Builds used the team-v2 automatic focused mutex and the
actor worktree's private target. Frozen executables are retained under
`local\act06-pack-frozen-tools-20261004-01` for subsequent coordinator review.

Original comparisons agree in cold, warm and reordered phases on 4,885 winning
packages, 88,491 fields, 9,770 scalar occurrences and 1,455,709 decoded bytes with
zero package findings. These include 13 PKDT8, 4,872 PKDT12 and 4,885 PSDT8 fields.
An additional complete-projection audit checks every earlier source section against
the preserved committed RACE comparison; the XLKR/RACE projections and known
deleted voice finding remain unchanged.

Independently authored comparisons agree in all phases on 14 winners, including
an unread version-99 tombstone, 27 fields, 24 scalars, 438 decoded bytes and six
findings. They exercise both layouts, signed extremes, raw unknown types/flags,
unused bytes, absent tails, physical order, duplicates/missing fields, extended
opaque fields and master/self winner identity. Both readers reject 17 malformed,
unsupported-version or hostile compressed inputs for their intended diagnostics.
Five altered reports (schedule byte, fabricated legacy tail, unused tail byte,
winning header and field offset) fail the complete comparison. The default
scalar-only projection remains equal without the optional package section.

Evidence is at `local\act06-pack-comparison-20261003-01`,
`local\act06-pack-authored-comparison-20261003-01`,
`local\act06-pack-authored-inputs-20261003\negative-results` and
`local\act06-pack-validation-20261003-01`. Completed comparisons bind the actual
dirty source manifests and frozen executable hashes before and after execution.
The handoff maps that tested code to the resulting commit; this documentation's
validation results were filled after comparison. These worker source checks do
not initialize actors, implement scheduling or accept gameplay parity.

## Actor model dependency sources (ACT-07)

Team-v2 decisions `ACT-07-source-manifest-v1` and
`ACT-07-manifest-API-detail` approve
`actors::dependencies::Catalogue::load(&mut RecordStore, &actors::Catalogue,
&actors::associations::Catalogue, &leveled::Catalogue, Limits)` and
`catalogue.manifest(&FormKey actor_root, &ArchiveAssets, ManifestLimits)`.
`actor-sources --include-dependencies` exposes the complete source catalogue and
existing inventory/list/template graph. Repeat `--dependency-root PLUGIN:LOCAL`
for explicit nondeleted NPC_/CREA roots. Local IDs are hexadecimal and exclude
load-order bits. Minimal CLI option/dispatch/finding wiring is declared for
integration. World can consume this API after integration; runtime retains
canonical actor state and saves, and assets retains animation decoding/playback.

| Source | Exact selected fields |
| --- | --- |
| NPC_ | MODL single terminated byte string; KFFZ physical terminated string frames; repeated PNAM4 to HDPT; HNAM4 to HAIR; ENAM4 to EYES; RNAM from the existing association catalogue |
| CREA | MODL single terminated byte string; NIFZ and KFFZ physical terminated string frames |
| RACE | NAM0/NAM1/MNAM/FNAM zero-byte markers, INDX4 unsigned raw index; contextual MODL/ICON strings; HNAM/ENAM arrays of complete four-byte hair/eye links |
| HDPT | MODL single terminated byte string; repeated HNAM4 extra-head-part links |
| HAIR | MODL and ICON single terminated byte strings |
| EYES | ICON single terminated byte string, with absence preserved |

Every word is little-endian. NPC_ versions 14/15, CREA 9/11/13/14/15,
RACE/HDPT/HAIR 15 and EYES 3/14/15 were observed in the installed winning cohort.
Other nondeleted versions fail explicitly. Deleted winners preserve their exact
header without version migration or body access. Physical list frames include
empty values, including a final empty frame in a two-NUL ending; zero-byte list
fields contain no frames. Single strings reject missing/embedded NUL bytes.
No missing path, model list, animation list or part is synthesized.

RACE marker occurrences carry their field index, decoded header offset and raw
index. Region changes reset sex/part context; sex changes reset the part. Hair,
eye and FaceGen declarations end the part context, so later FaceGen markers
cannot inherit an earlier head/body part. Unscoped paths remain present with a
finding. Duplicate singleton model/texture/hair/eye/list fields remain present
with findings. Repeated head-part links and contextual RACE models are ordinary
physical occurrences. Unknown fields keep their signature, extent, offset and
SHA-256, with the complete body retained.

The pinned xEdit FNV declarations are CREA4276-4423, NPC_6798-6955,
EYES4710+, HDPT4840+, HAIR6346+ and RACE7351-7469. Common8834-8870
defines the head-part model/texture fields; Interface5323's `wbIsFallout3`
includes FNV. Therefore the separate later-game HEAD-link branch is not used.
The Common and Interface hashes are respectively
`e616b6546f6df74d88ec98bb870906db380c985963d011e4931c5726682b7d26`
and `eae6a304e3f1f69cc030eca645d768e9f20fd2cb9c5ffe0f3a2a457c9e51e358`;
the FNV revision/hash remain those cited above. A private original-byte matrix
independently matches all 31 preserved RACE body hashes and records 556 contextual
ICON and 682 MODL occurrences.

Catalogue joins compare normalized source names, full source byte/hash receipts
and canonical winning-content digests before joining bodies. Recovered checksum
faults cannot supply typed dependency inputs, even when their receipt/header
joins match. NPC_/CREA bodies are borrowed from the existing scalar/inventory
catalogue. RNAM occurrences are
borrowed semantically from the existing association decoder, while the new
catalogue retains its own bounded binding projections. Source limits precede
record keys, reads/inflation, aggregate decoded bytes, fields, string frames,
raw path copies and bindings. The inventory/list graph and iterative SCC helper
are reused rather than implementing an alternative production traversal.

Each manifest retains every physical model link and the existing inventory
closure edge indices, including null/missing/deleted/wrong-kind targets.
Traversal visits each retained source identity once and expands only defined
model links of the evidenced target kind. Inventory graph traversal retains
its existing structural semantics. Combined SCCs index the sorted retained root
nodes and do not claim whole-corpus analysis of untraversed wrong-kind targets.
Manifest budgets precede node/edge/path/candidate copies and graph work.
The CLI admits at most 64 roots before opening inputs and consumes one aggregate
manifest budget across all roots, including repeated roots.

MODL lookup follows the existing world's `meshes/` prefix convention. ICON
lookup reuses `vfs::texture_path`. Authored raw bytes remain separate from safe
normalized lookup keys. Empty, unsafe and paths longer than 4096 bytes receive explicit
lookup states. Every matching physical archive candidate is retained; collisions
do not select a winner. NIFZ/KFFZ relative bases remain explicitly unresolved,
including bare filenames observed in the installed source. No directory is
guessed, no archive payload is decoded, and no inheritance, equipment, gender,
part/clip selection, scheduling, AI or gameplay acceptance is implemented.

The independent projection retains the strict native actor reader and augments
it with `tools/actor-oracle/dependencies.py`, a separate standard-library raw
plugin and BSA metadata reader. It checks the native source cohort, retains
write-denying source handles, bounds compressed lengths before inflation and
uses iterative Tarjan SCCs independently of the production Kosaraju helper.
The wrapper hashes the companion script and Python interpreter before and after
comparison, polls owned processes for STOP, and retains native base outputs.
Use absolute input paths, as the wrapper does, to compare exact container names.

Final private gates pass all 56 actor tests, including the reachable forensic
checksum recovery regression, affected data/CLI all-target Clippy with warnings
denied, CLI build, Rust formatting, PowerShell/Python syntax and whitespace checks.
Sealed tools in `local/act07-dependencies-frozen-tools-20261004-02` match complete
independent original and authored projections in cold, warm and reordered runs.
The final original catalogue contains 6,626 winners, 209,224 physical fields,
15,255 string frames, 12,433 bindings and no dependency source findings. Its
existing inventory graph contains 14,185 nodes and 56,193 edges. Explicit root
`FalloutNV.esm:7` retains 128 single archive candidates; creature root
`FalloutNV.esm:17A03` retains two single candidates, four empty paths and 50
unresolved relative paths.

The authored cohort contains 11 winners, including an unread version-99 HDPT
tombstone, 52 fields, 17 string frames and three source findings. It exercises
overrides, self selectors, cycles, wrong/missing/deleted/null links, empty and
legacy strings, archive collisions and unsafe/overlong paths. Both readers reject
13 malformed or unsupported cases for the intended diagnostic; the CLI rejects
11 altered complete dependency projections. A 1,026-node root is admitted;
64 repeats exceed the shared 65,536-node report budget in both readers, and the
65th root is rejected before opening inputs. The default actor report is exactly
byte-equal to the preserved PACK executable on the authored cohort.

Final comparison receipts are under `local/act07-original-comparison-20261004-02`
and `local/act07-authored-comparison-20261004-03`; negative, root-budget and default
receipts are under `local/act07-negative-results-20261004-04`,
`local/act07-root-budget-results-20261004-03` and
`local/act07-default-results-20261004-01`. The handoff in
`local/act07-dependencies-validation-20261004-01` binds the tested source/tool
hashes to the two-commit range. Only this documentation's validation results were
filled after the comparisons. Earlier prototypes and failed fixture setup or
admission tests remain preserved separately. These receipts establish source
decoding and engineering bounds; no gameplay scenario is accepted.

## Forensic source admission repair (ACT-07D)

Reachable `RecordStore::open_nv` checksum recovery originally allowed the RACE
and PACK scalar catalogues to accept typed fields from a recovered body marked
with `integrity_issue`. Authored compressed records reproduce both gaps while
preserving the exact recovered payload. The existing strict mode rejects the
same corrupted inputs. Each scalar decoder now rejects the integrity marker
before visiting any fields and reports the source filename, record offset and
specific race/package source-admission reason. Recovery remains diagnostic;
it does not repair or authorize source data.

All 58 actor tests pass, including both captured admission regressions and exact
valid-source projections across strict and forensic modes. Affected all-target
Clippy with warnings denied, CLI build, formatting and whitespace checks pass.
The sealed repaired binary produces the same complete 318,736,895-byte installed
source report and 14,913-byte default authored report as the preserved ACT-07
binary. The installed report also matches the preserved independent complete
source oracle. Receipts remain under `local/act07d-trust-validation-20261004-01`
and `local/act07d-good-source-results-20261004-01`; the binary is sealed under
`local/act07d-trust-frozen-tools-20261004-01`. No source schema, runtime state,
save format, scheduling or verified gameplay behavior changes.

## Physical package dependencies (ACT-07B)

`actors::package_dependencies::Catalogue::load` joins the existing PACK scalar
catalogue and loaded script catalogue to the current winning store. It checks
complete source cohorts, winning-content identity, each header/body digest and
every borrowed script version. Deleted winners retain their headers without
reading bodies. Recovered bodies cannot supply dependency inputs.

Physical CTDA occurrences use the scripting lane's `prepare_record` decoder,
including exact 20/24/28-byte layouts, preceding field offsets, preserved raw
words and explicit unknown dependencies. No owning condition group or truth
value is inferred. SCHR units borrow the existing script handles, version,
declarations, ordered references and issues. PACK script ownership remains
`unverified_embedded`; nearest POBA/POEA/POCA markers are recorded separately.
Pinned xEdit declares these event markers as empty, INAM as an IDLE/NULL FormID
and TNAM as a DIAL/NULL FormID, each exactly four bytes. Missing markers and
wrong target kinds receive source findings. Link resolution reuses the existing
inventory binder; null, missing, deleted and wrong-kind targets stay distinct.

The exact raw installed-source matrix contains 4,885 PACK records, 88,491 fields,
3,801 CTDA occurrences (24 width-20 and 3,777 width-28), 14,616 SCHR units,
558 SCDA bodies, 595 SCTX fields, 875 SCRO and 10 SCRV references, and 13
SLSD/SCVR declarations. Each marker occurs 4,872 times; INAM and TNAM each
occur 14,616 times. All decoded body hashes match the preserved native oracle.
The matrix is retained under `local/act07b-pack-matrix-20261004-01`.

Admission limits cover 65,536 package records, 64 MiB per record, 256 MiB total
decoded bytes, two million physical fields, four million visits over two passes,
one million condition sites, 128 MiB of condition retained rows, one million
event fields and links, 262,144 script units and declarations, one million
references, and 128 MiB of compact serialized definitions. Counts are charged
across records before retaining further views. Compact projection bytes cover
definitions rather than total process heap, prior catalogues, descriptor
receipts or pretty report bytes; borrowed script metadata is not cloned.

`actor-sources --include-packages --include-package-dependencies` is the optional
consumer. It reuses the fingerprinted command catalogue and reports descriptor
provenance separately: plugin source receipts do not certify caller-supplied
signatures. The independent raw Python reader in
`tools/actor-oracle/package_dependencies.py` reads locked plugin bytes and the
same exact executable's descriptor bytes. It augments the existing native
oracle without invoking production decoders or executable handlers. Default
actor reports keep their existing schema and bytes.

Exact approved scripting prerequisites are VM-03 `5a505ccb` mapped as `de5fb78`
and VM-03B `0a16e72` mapped as `b1a8d5e`. The latter preserves the exact patch
while excluding the unrelated earlier `decoded_record_bytes` getter. No other
script changes were adopted. This slice does not initialize actor state, infer
event ownership, execute conditions/scripts, choose schedules, or establish AI
or gameplay parity.

All 65 actor tests pass, including seven new source-join, exact/one-less bound,
deleted-body, aggregate-record, warm-reload and dependency-only binding cases.
Affected data/CLI all-target Clippy with warnings denied, CLI build, formatting,
PowerShell/Python syntax and whitespace checks pass. Complete installed and
authored projections match the independent reader in cold, warm and reordered
runs. Installed dependencies retain 3,801 conditions, 14,616 scripts, 558
compiled bodies, 13 declarations and 885 references, with no new source findings.
The authored four-package cohort includes an unread deleted winner, four
conditions, four scripts, seven references and eleven explicit source findings.

Both readers reject fourteen malformed source cases for their specific field
diagnostic. The CLI rejects fifteen altered complete projections, refuses an
unsupported executable fingerprint, and requires the package scalar flag before
opening inputs. The default authored report is byte-equal to the preserved
ACT-07D binary. An extra raw tag containing space, NUL and `0xFF` exposed display
label escaping in the independent reader; the corrected reader matches the full
crafted projection. Corrected installed oracle bytes are equal to the sealed
original oracles for both load orders. No Rust decoder changed for that fix.

Original comparison receipts are in `local/act07b-original-comparison-20261004-01`;
final authored receipts are in `local/act07b-authored-comparison-20261004-02`.
Corrected-reader equality, tag-label and negative/default receipts are in
`local/act07b-reader-equality-20261004-01`,
`local/act07b-tag-label-reports-20261004-01` and
`local/act07b-negative-results-20261004-01`. Final tools are sealed in
`local/act07b-package-frozen-tools-20261004-02`. The installed comparison used the
preceding reader, and the equality receipts bind its complete oracle bytes to
the corrected reader; final authored runs use the corrected reader directly.
Earlier pilots, fixture setup failures and intermediate private helper drafts
remain separate. These are source-decoding and engineering-bound receipts,
with no actor initialization, scheduling, AI, save or gameplay acceptance.
## Explicit equipment source roles (V3-ACT-08)

`actors::dependencies::equipment::request` accepts an explicit actor root,
equipment FormKey and source role. It checks the actor/store's exact ordered
source receipts and winning-content identity before reading the selected winner.
The existing physical field decoder retains every field hash and source offset.
The default actor dependency catalogue remains unchanged; inventory presence does
not select equipment, and an explicit choice can request an item outside CNTO.

Armor and armor-addon requests distinguish male/female biped MODL/MOD3 and world
MOD2/MOD4 declarations. Weapon requests distinguish the model modification mask
0–7 (MODL/MWD1–7), shell MOD2, scope MOD3, world MOD4, and first-person mask 0–7
(WNAM/WNM1–7). A unique defined first-person STAT link is followed only after its
winning target kind/plugin/offset/flags match. Repeated links remain ambiguous
and are not followed; absent, null, missing, deleted and wrong-kind inputs retain
their source bindings. Missing female models do not acquire a male fallback.

Complete selected records retain BMDT's raw slot/general flags and unused bytes,
signed ETYP, model paths, first-person bindings and opaque texture-swap fields.
The pinned xEdit FNV schema at revision
`9fb016884bec138ea6c7b872cec831537d464c3e` supplies BMDT at 3898–3932,
armor/addon roles at 3934–4037, ETYP at 3464–3465 and weapon roles at 8625–8705.
Common 9512–9543 supplies textured model filenames. This slice admits observed
version 15 source bodies; older STAT versions explicitly refuse. Slot conflicts,
active weapon modifications, equipping, texture swaps, attachment node selection,
NIF decoding and effective retail model choice remain unsupported.

The producer reads at most the selected equipment and one singleton STAT. It
bounds stored/decoded record bytes, physical fields, path bytes/strings, bindings,
selection visits, requests/links/issues and aggregate archive candidates. Model
lookup reuses the existing actor manifest path admission and `ArchiveAssets`;
empty/unsafe/long/missing/colliding paths remain precise source requests.

The headless consumer requires exactly one actor dependency root and both choice
arguments. For example:

```powershell
.\target\debug\fallout.exe actor-sources --install LOCAL_INSTALL --load-order ORDER_JSON --include-dependencies --dependency-root ORIGIN:ACTOR_ID --equipment-source ORIGIN:ITEM_ID --equipment-role armor-female-biped --output local/equipment.json
```

Other role strings are `armor-male-biped`, `armor-male-world`,
`armor-female-world`, `weapon-model:0` through `:7`, `weapon-first-person:0`
through `:7`, `weapon-shell`, `weapon-scope` and `weapon-world`. Source requests
do not establish equipped state or renderer readiness. The optional equipment
projection is checked in full by `--compare-oracle`; default reports are preserved.

Validation on 2026-10-04 passed six equipment tests plus 21 existing dependency
tests (one unrelated installed test ignored), format, data/CLI warnings-denied
Clippy and the CLI build. All 27 authored equipment choices matched complete
independent raw plugin/BSA projections through the frozen CLI; invalid required
arguments/masks/multiple roots and altered source paths rejected. Exact limits,
current armor/STAT overrides, signed/raw slot extremes, absent equipment and
duplicate paths/links were checked. No gameplay behavior is accepted by this work.

Two complete installed CLI projections also match fresh independently decoded
equipment requests for actor `FalloutNV.esm:104C0C`: an explicitly chosen female
biped model from `OutfitJessupBandana` (`17409D`) and an explicitly chosen
first-person model from `WeapShotgunSawedOffNPC` (`17B7B8`) through its winning
STAT. They retain one and two source records respectively, with one request,
one archive candidate and no source issues each. These choices do not imply
that the selected actor wears or carries those items. The base ESM source audit
observed all 389 ARMO, 131 ARMA and 261 WEAP headers at version 15, alongside
older STAT versions that remain unsupported by this slice.

## Selected render request admission (V3-ACT-11)

The existing `render_manifest` now reports `selected_source_cycles`, with indices
in its ordered `sources`, and `selected_requests_admitted`. The latter admits only
the selected physical source requests: at least one request, no source issues or
selected cycles, no ambiguous selected declaration, and exactly one archive
candidate for every selected path. It does not decode NIF bytes, validate textures,
compose FaceGen, choose equipment, or establish renderer/gameplay readiness.

Cycle analysis reuses the shared graph helper over the exact admitted actor and
extra-head-part links. Broad inventory, template and race option edges cannot
become selected render links. The new node/edge work consumes the existing render
visit budget, and every node of a selected cycle produces a bounded
`cyclic_selected_render_source` issue. Cycles retain their original source headers,
links, offsets and path requests rather than being removed or chosen by traversal
order. Missing, inherited and repeated declarations keep precise diagnostics.

The existing authored head-part cycle `110 -> 111 -> 110` now reports the selected
cycle and refuses admission. A separate authored fixture independently selects
male race A/body/skeleton source declarations, then a winning compressed actor
and race override selects female race B and different skeleton/body paths. Its
expected source offsets come from the authored file layout. A unique archive
candidate cannot admit duplicate model declarations, unknown template selection
or missing candidates. Archive metadata fixtures contain no decoded NIF models.

Validation on 2026-10-04 passed 22 dependency tests plus six equipment tests (one
unrelated installed test ignored), format, warnings-denied data/CLI Clippy and
the CLI build. Complete base, override and cyclic-source CLI projections match
the independent native scalar and Python physical dependency/render readers.
The cyclic fixture retains its existing scalar-source findings and exits 1 after
emitting its matched report; the clean base and override cases exit 0. A changed
admission flag fails the actual CLI comparison. Earlier compilation attempts and
all immutable input/output receipts are preserved under
`local/v3-act11-render-admission-20261004-01`.

The installed `FalloutNV.esm:104C0C` projection also matches the full independent
reader: six selected sources, 26 requests, no selected cycles or source issues,
and source-request admission true. Its structural source/archive manifest was
freshly checked against the preserved independent baseline, and all selected
physical source bodies/field origins were freshly verified. All 31 original
plugin/archive inputs and five private frozen/baseline inputs keep their hashes.
This source admission establishes no renderer or gameplay acceptance.

## Selected voice source requests (V3-ACT-12)

`actors::voices::request` joins one explicit actor to its existing authored
associations and race catalogue. Exact ordered plugin receipts and winning
content identity must match the supplied store before any target read. Every
actor VTCK/RNAM occurrence keeps its physical field and source binding. A unique
nondeleted RACE link exposes both authored VTCK FormIDs, in male/female order;
their alignment with a unique NPC ACBS sex declaration is reported separately.
CREA flag bit zero never supplies an NPC sex. Traits inheritance, missing or
repeated configuration leaves authored sex unavailable.

Repeated singleton actor/race links stay ambiguous and are not followed. Null,
missing, deleted and wrong-kind target bindings stay explicit. At most three
unique winning VTYP sources are followed; their complete headers, physical
field hashes/offsets and optional raw DNAM flag bytes are retained. Repeated
DNAM fields remain separate and receive a diagnostic. Unknown bits are retained.
Unsupported VTYP header versions retain their identity with an unread body;
deleted compressed tombstones are never decoded.

The pinned xEdit FNV source at `9fb016884bec138ea6c7b872cec831537d464c3e`
declares VTYP's optional one-byte DNAM at 6094-6101 and RACE's two VTCK FormIDs
at 7400-7403. The independent local base-ESM check observed voice versions
1, 4, 9, 11, 13, 14 and 15. All nine version-1 records omit DNAM; all 91 later
records contain one byte. Absence remains absence. Default dialogue flags do
not evaluate dialogue conditions, select a fallback voice or choose an audio
language/path. This consumer does not load audio or initialize actor behavior.

```powershell
.\target\debug\fallout.exe actor-sources --install LOCAL_INSTALL --load-order ORDER_JSON --voice-root FalloutNV.esm:104C0C --output local/voice.json
```

The explicit `--voice-root` enables `actor_voice_requests` and loads the existing
association/race prerequisites. Their whole catalogues appear only with the
existing `--include-associations`/`--include-races` options. Default source
reports are unchanged. `--compare-oracle` checks the complete new projection.
The independent `tools/actor-oracle/voices.py` reuses the raw source/header
reader and checks native actor/race/association rows against fresh physical
bytes before deriving voice pairs, flags, provenance and sex alignment.

Limits bound individual and aggregate decoded bytes, physical fields,
declarations, selected sources, visits, issues and the complete serialized
projection. The existing strict indexed reader also bounds a compressed
record's stored extent by the remaining decoded allowance; it may conservatively
refuse a tightly bounded compressed record. Existing actor/race decoders remain
the only decoders of their typed scalar fields.

Validation on 2026-10-04 passed eight voice tests, seven association tests and
six race tests, format, data/CLI all-target warnings-denied Clippy and the CLI
build. Nine complete authored native/raw/CLI projections match, including
compressed winning overrides, repeated declarations, null/missing/wrong-kind
targets, unread deleted/unknown-version bodies, traits requirements and creature
flags. The NPC fixtures retain the existing missing-class finding when the
whole association catalogue is requested; their reports still compare exactly.
The voice-only consumer exits zero, default scalar reports match, and changed
voice flags, raw bindings and sex alignment reject through the actual CLI.

The selected installed Doc Mitchell (`FalloutNV.esm:104C0C`) request also matches
the full independent reader. It retains three winning VTYP sources (versions
15, 4 and 9, raw DNAM bytes 0, 1 and 3), four declarations, 175 physical fields
and no voice issues. All ten original plugin and seven private input hashes
remain unchanged. Evidence and failed drafts remain under
`local/v3-act12-voices-20261004-01`; no gameplay or audio behavior is accepted.
# Selected authored package destination operands

`actors::packages::destinations::request(&mut RecordStore, &packages::Catalogue,
&FormKey, Limits)` borrows one winning physical PACK definition and returns every
PLDT, PLD2, PTDT and PTD2 occurrence with its field index, decoded offset and raw
payload. Signed discriminants and Radius/Count-Distance words stay exact; the
optional target float is represented by its original bits. The complete pinned
FNV PACK schema and common union decider establish the admitted layouts:
locations have 12 bytes, targets have 12 or 16. Other layouts remain opaque.

Only schema-confirmed reference, cell and object-ID alternatives use the existing
store binder. Location and target object sets differ; target one permits IDLM,
target two does not. Object-type words and unused/context alternatives never
become FormIDs. Null, absent, deleted, wrong-kind, unknown and repeated operands
remain visible; every repeated occurrence loses binding admission. A unique
live permitted binding admits a source operand only. No path target is selected,
no radius or count is interpreted, and no nearest-object search, conditions,
schedule or AI execution occurs.

The optional `actor-sources --include-packages --package-destination PLUGIN:HEX`
consumer emits `actor_package_destination`. Existing package projections remain
unchanged. Source receipts, winning identity and header must match the existing
catalogue. Source count, selected physical field visits, operand count, aggregate
raw bytes and compact projection bytes have independent bounds. The separate
`tools/actor-oracle/package_destinations.py` reads union payloads directly from
source bytes and compares the full request, including every physical occurrence.

## Actor object-script attachment request

`actors::script_attachment::request(&mut RecordStore, &actors::Catalogue, &loaded_scripts::Catalogue, &FormKey, Limits)` returns an immutable, non-deserializable source request. The mutable store borrow serves its existing source digest cache; the request verifies all three exact ordered source cohorts and winning identity before using the retained actor bytes or canonical FormID binder. It preserves every physical SCRI index, offset and raw byte, every ACBS configuration declaration, and the raw Script template bit (512).

A unique live SCPT link can select one existing standalone compiled definition with verified source ownership. Its handle, version, owner, raw script type/flags, complete declaration and reference tables and source issues borrow the loaded catalogue. Repeated links, malformed widths, null/missing/deleted/wrong-kind targets, absent or multiple units, absent compiled bytes, missing/repeated configuration and script inheritance leave selection unavailable. Repeated variable declarations remain ordered; no new first-wins declaration lookup is introduced. Raw script flags and type do not determine original enablement or execution.

`actor-sources --script-root FalloutNV.esm:104C0C` adds only `actor_script_attachment.request`; the default report remains unchanged. The selected work is bounded by sources, physical field visits, attachment count/raw bytes, matching compiled units, declaration/reference counts and full serialized request size. The independent `script_attachment.py` oracle reuses both existing native source producers and the existing independent script-unit helper, reading fresh source bytes and never deriving expected fields from the observed report. This producer creates no runtime instance, event list, queued event or actor state; template inheritance and gameplay execution remain unsupported.

## Authored AI and combat-style inputs

`actors::ai_inputs::request(&mut RecordStore, &actors::Catalogue, &FormKey, Limits)` prepares exact physical AIDT declarations and explicit ZNAM links. Protected source receipts and winning headers must match the existing actor catalogue; retained source identity is checked separately, so caller-modified receipt rows cannot relabel old bytes. The store borrow changes only its existing digest/cursor cache.

For the already observed actor header versions, a twenty-byte AIDT exposes raw aggression, confidence, energy, responsibility and mood, all three unused mood bytes, service flags, signed teaching/assistance bytes, training level, raw aggro-radius boolean and signed radius. Known enum membership is separate from the raw word. Every repeated occurrence remains visible. Unknown version/layout/enum, missing or repeated configuration, and the AI Data template bit (16) prevent admitting a unique authored AI input. Raw service flags and unused bytes remain uninterpreted; no service, training, hostility or radius behavior is evaluated.

The independently gated Traits template bit (1) governs admission of a unique explicit ZNAM link to a live CSTY winner. The request exposes its existing FormID binding and full winning header without reading combat-style bodies or formulas. Null/missing/deleted/wrong-kind and repeated links stay visible and unavailable. A caller can request `actor-sources --ai-root PLUGIN:HEX`; the complete manifest is compared when an independent oracle is supplied, while default actor/stat/faction reports stay unchanged. Sources, physical visits, selected fields, retained raw bytes and full serialized output are bounded. No actor value, event, navigation request, mutable World or AI execution is created.

## Selected race and class initialization inputs

`actors::initialization_inputs::request(&mut RecordStore, &actors::Catalogue,
&associations::Catalogue, &races::Catalogue, &classes::Catalogue, &FormKey, Limits)`
joins the existing typed producers over one exact ordered source cohort. Its
opaque manifest cannot be deserialized or constructed from an inspection report.
Every physical NPC RNAM/CNAM occurrence retains its association, scalar field,
raw bytes, index and offset. Live permitted targets borrow the complete existing
RACE/CLAS definitions and full headers; deleted typed targets retain tombstones.
Wrong-kind links never select a different producer. CREA RNAM/CNAM have different
source roles and produce no race/class links.

Repeated links, missing or repeated ACBS, and the Traits template bit leave direct
binding unavailable. A unique direct link remains distinct from complete decoded
target inputs: missing or repeated RACE DATA/PNAM/UNAM or CLAS DATA/ATTR prevents
input completeness while preserving every declaration. Both sex height/weight
arrays, signed skill/tag/training words, raw flags, unused bytes and float bits
remain exactly as supplied by the existing decoders. No template is followed,
effective race/class selected or actor value initialized.

`actor-sources --initialization-root PLUGIN:HEX` emits the optional complete
`actor_initialization_inputs.manifest`, with exact independent comparison.
Sources, conservative physical visits, all retained actor/configuration/target
fields, link count, decoded bytes and compact projection have separate limits.
The independent oracle composes existing native actor/association/RACE/CLAS and
inventory producers after fresh source/header/body/field validation. Default
actor and ACT09 reports keep their existing projections.
## Effect declaration source inputs

`actor-sources --effect-root PLUGIN:HEX --effect-field INDEX` requests one exact
physical SPLO or EITM occurrence. The request joins the existing actor and
association catalogues to the winning SPEL or ENCH record and explicit EFID
groups. It retains every physical declaration field, byte span, digest and raw
payload, including unsupported versions and layouts. The tested typed profile
is record version 15: SPIT/ENIT are 16 bytes, EFID is four bytes and EFIT is 20
bytes. EFIT magnitude, area and duration stay unsigned words; actor value stays
signed. Metadata flags and unused bytes remain raw.

Each EFID starts a separate ordered group. Repeated links remain distinct.
Missing or repeated EFIT, out-of-order members, unknown effect types and malformed
layouts remain explicit. CTDA fields retain their exact ordered bytes; this
request does not evaluate conditions. MGEF requests contain winning headers and
source receipts without reading or interpreting MGEF bodies. It does not apply
xEdit's editor-time actor-value rewrite.

SPLO and EITM use the ActorEffectList template bit (8). Missing or repeated ACBS,
template inheritance and repeated singleton EITM preserve their source evidence
while withholding binding admission. Null, missing, deleted and wrong-kind links
retain their binding status. Exact source cohorts and root bytes are checked;
source count, depth, selected records, record bytes, decoded bytes, field visits,
retained fields, bindings, groups, raw bytes and projection bytes are bounded.
Typed source admission does not establish an active effect, condition truth,
stacking, timing or gameplay behavior. Execution remains unavailable.
