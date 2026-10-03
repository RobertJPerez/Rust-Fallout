# Immutable actor source fields

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
