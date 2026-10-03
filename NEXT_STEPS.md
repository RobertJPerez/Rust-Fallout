# Resume here

The next unmet gate is **M1: trustworthy, semantically useful content loading**.
The current tools build and run; the original games have not been recreated yet.
Keep the entire master brief in scope and progress through its dependency gates.

Checkpoint 14 is ready for a direct visual/input comparison. Run the verified build
with [these test instructions](docs/terrain-textured-preview.md). Windows UI
automation failed to connect to its native pipe after retry/reset, so original-game
camera/material comparison is an actual manual test gap, not missing file access.
Automated data, cache and GPU checks pass; no retail/gameplay gate is accepted.

1. Resolve the three documented vanilla format exceptions with independent evidence.
   Reproduce the strict LAND failure with:

   ```powershell
   .\target\release\fallout.exe census --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
   ```

   It currently fails at LAND `00150FC0`, offset `0x0B0CFF04`. Diagnostic mode writes
   the census and returns 1. Compare the two Misc archive payload failures using
   `tools/compare-corpus.ps1`. Design digest-scoped compatibility only after verifying
   the expected handling; do not rewrite the installation or silence all zlib errors.

2. Capture the original game's effective profile and a minimal isolated retail oracle
   scenario. Windows, the installation, and hardware are available; this is unfinished
   work, not a request for additional user access. Keep test profiles and captures
   separate from existing saves. Obtain xEdit per-record/reference exports for selected
   cells/scripts, then compare the runtime's decoded fields with those exports.

3. Implement archive/loose lookup precedence and invalidation fixtures for that profile.
   The current VFS reports 420 cross-archive path collisions and intentionally has no
   production precedence policy. The on-demand record store and typed CELL/REFR/ACHR/ACRE
   dependency inspection now work, including winning parent membership and target-kind
   checks. Checkpoint 08 now persists source-bound plugin metadata with verified
   cold/warm reuse and process-termination recovery. See
   [record index caching](docs/record-index-cache.md). Winners still rebuild for the
   supplied order, and deferred payloads remain strict on access. Extend this to
   a canonical content store and remaining fields. Checkpoint 09 now inspects
   WRLD and LAND source fields for Goodsprings and four DLC exteriors, preserving
   height deltas, raw normals/colors, layers, parent flags and unknown bytes.
   See [exterior inspection](docs/exterior-fields.md). Checkpoint 10 reconstructs
   VHGT under a pinned ESM4 model and diagnoses four Goodsprings boundaries. See
   [terrain heights](docs/terrain-heights.md). Measured retail heights/axes/units,
   world-parent inheritance and retail rendering are open. Checkpoint 11 builds
   source meshes and renders five real terrain cells through the Bevy inspector.
   See [terrain geometry](docs/terrain-geometry.md). Normals, winding and color space
   follow an explicit inspection model; retail comparison, normal repair,
   landscape textures/blending, water, props and streaming remain unfinished.
   Extend full asset dependency closure and record-specific override rules.
   Checkpoint 12 now follows authored nonnull terrain layers through winning
   LTEX/TXST records to unique texture archive members, with independent field/byte
   comparisons and cache verification. See [terrain texture dependencies](docs/terrain-textures.md).
   Four NULL default layers remain explicit/unapplied; defaults, world inheritance,
   grass assets/rules and texture pixel/blend behavior still need evidence.
   Checkpoint 13 expands authored alpha samples into a bounded quadrant blend model
   and compares every byte with an original C++ calculation. See
   [terrain blend maps](docs/terrain-blends.md). Missing bases, NULL defaults,
   overfull coverage and source values remain explicit; retail blending is open.

4. Continue NIF payload decoding with remaining scene-node kinds, skinning and
   collision. Sixteen authored collision block types now decode in Rust; the house's
   731 supported collision blocks match the raw oracle exactly. Shape links are
   checked as a DAG, allowing sharing. Constraints/controllers, measured Havok units,
   body/placement conversion and physics queries remain open. See
   [collision decoding](docs/nif-collisions.md). Geometry and nine material/texture types
   also decode in Rust. The
   independent raw-factory comparison covers 218 files, 751 objects, 403 meshes
   and 1,127 material blocks. The full payload scan decodes 176,982 material blocks
   in 25,729 files; 25 additional files still fail float/index checks. Seven texture
   paths in six DLC models retain absolute exporter prefixes and remain unresolved.
   Inspect `reports/nif-materials.json`, `reports/nif-scenes.json` and their docs for evidence and
   remaining gaps. Do not disable numeric/index checks globally to admit bad data.
   The corpus now has a complete archived NIF/KF inventory: 25,754 decoded containers,
   159 block types, and six unsupported 20.0.0.4 files. Independent container checks
   cover 218 files and 4,582 blocks. The six legacy files need a separate version path;
   do not pretend their missing block-size table matches 20.2.0.7.
   Extend census to UI operators, audio codecs, and actual SCDA instructions/events.
   Checkpoint 15 now frames all 14,514 compiled bodies and independently compares
   142,218 headers, 28,463 reference calls and 5,702 event headers. See
   [compiled script inspection](docs/compiled-scripts.md). There are 217 top-level
   native command IDs; expression calls are still uncounted. The 148 condition IDs
   retain their separate identifier space. Continue reference/variable binding,
   expression and native argument decoding, registry evidence and execution.
   Checkpoint 16 independently reads the exact executable and compares 640 native
   command, 38 event and 16 statement descriptors, including 638 parameter
   occurrences. All 217 observed top-level commands, 33 events and 148 condition
   IDs bind to metadata; native behavior is unimplemented. See
   [command catalogue](docs/command-catalogue.md). No script is executed yet.
   The script table layer now preserves 80,736 units and compares all 28,463
   top-level caller associations. Three stale empty-unit reference counts and
   eleven duplicate variable indices remain documented, with no source repair.
   See [script table bindings](docs/script-bindings.md). Continue expression and
   native operand decoding; stable winning embedded-script identity and loaded
   reference values remain unverified.
   Expression inspection now compares 53,404 envelopes and 149,082 tokens from
   all 14,514 compiled bodies. All 143 embedded command IDs bind to vanilla
   metadata. See [compiled expressions](docs/script-expressions.md). Native
   arguments now decode across 67,931 calls, with 80,377 regular operands and
   75 message substitutions. ShowMessage trailing words remain uninterpreted;
   see [native operands](docs/native-arguments.md). Operand table associations now match independently across 159,111 uses;
   25,560 foreign locals retain deferred target declarations. See
   [operand associations](docs/operand-bindings.md). Continue loaded target lookup,
   typed quest/dialogue ownership, postfix structure and control-flow/behavior evidence.
   The condition reader now preserves all 80,627 CTDA fields, including 125 short
   layouts and two records with unverified fields. See
   [condition fields](docs/condition-fields.md). Parameters, grouping, subject
   resolution and query evaluation remain open. Typed QUST/INFO/DIAL structure now
   preserves 51,135 records, 176,792 sections and 59,225 script owners. Two short
   quest headers and eighteen orphaned shared-info entries remain diagnostic;
   see [narrative structure](docs/narrative-structure.md). Continue canonical winning
   record/script identity and loaded target lookup. INFO parent GRUP labels and
   winning topic membership now match a direct original-header reader across
   all 629,788 definitions. Cache v2 adds the topic parent and retains fresh
   cold/warm and selected-cell comparisons; see
   [dialogue membership](docs/dialogue-membership.md). Retail INFO order, script
   identity/versioning and foreign declaration lookup remain unfinished.

5. Finish the real interior's rendering and measured acceptance, then Goodsprings.
   `fallout-preview --cell GSDocMitchellHouse` now assembles 400 of 435 references
   through strict selected-record reads, sharing 203 models and 146 texture/sampler
   pairs. Two source-coordinate GPU views were captured and inspected. See
   `docs/interior-preview.md`. Checkpoint 06 fixes the magenta window/shadow bindings
   and verifies alpha/culling/depth states with 62 synthetic GPU checks. Standalone
   marker models remain visible; NoLighting falloff/emittance, inherited properties,
   alternate item/actor models, retail lighting and physics are open. Checkpoint 07
   decodes authored collision separately; it does not constrain the inspection camera.
   Verify axes, units, placement rotation, winding, materials, collision, and
   camera movement against measured retail fixtures. No procedural replacement map or staged
   quest demo can satisfy M2/M3. Continue simulation, ObScript, saving, and campaign
   work in the brief's order with observable behavior tests.

   `GSDocMitchellHouse` is now a reproducible data fixture: 435 placed references,
   222 distinct base records, and 207 distinct model candidates checked against
   nifly for container, supported scene fields, geometry and materials. All 854 authored
   texture references resolve to 312 unique archived textures, cached and hash-verified.
   Use `fallout cell --inspect-models` as documented in
   [world inspection](docs/world-inspection.md). Its report still returns 1 for the
   known base-plugin integrity issue when fully scanning payloads. The new
   `--defer-unread-payloads` option validates selected reads without decoding unrelated
   bodies; 585,196 deferred bodies are explicitly unvalidated. It reproduces all
   selected source fields and returns 0 for this fixture. Physics is not implemented.

6. Extend the existing R3 preparation/cache modules into bounded, cancellable jobs with
   a journal and transitive dependency invalidation. Test process termination during
   extraction, conversion, and publication. Add profile-bound persistence when the
   first actual state exists; current cache tests are not save-system tests.

Use `tools/check.ps1` after code changes. The raw local evidence is under `local/`;
the public summary is [reports/checkpoint.md](reports/checkpoint.md). Update the parity
ledger and source pins with concrete evidence. All gameplay milestones and other-game
adapters remain open. Checkpoint 04 was uploaded as commit
`d33c7a11d3288a321604fad43b3021f28a7ed43b` to
[RobertJPerez/Rust-Fallout](https://github.com/RobertJPerez/Rust-Fallout). The current
verification report binds the tested implementation revision and source/binary hashes.

Checkpoint 24 adds the immutable winning script catalogue, versioned handles,
source owner metadata and explicit reference dependency states. See
[loaded scripts](docs/loaded-scripts.md). Next: associate static quest SCRI
definitions and resolve foreign declaration lookups where the source relation
is established. Placed-reference live event lists and SCRV values remain runtime
dependencies; do not substitute a base-object authoring link for a live script.

Checkpoint 25 adds static quest SCRI attachments and foreign declaration lookup.
All runtime values remain unresolved; placed references require live event-list
state. See [quest attachments](docs/quest-script-attachments.md). Next: independently
verify compressed record extraction and the original LAND checksum exception,
then continue typed conditions and the first event/runtime state slice without
claiming unmeasured original timing, defaults or command effects.

Checkpoint 26 independently verifies every compressed source record and isolates
the existing LAND checksum mismatch with equal payload hashes and Adler results.
Both strict readers still reject it. See [compressed records](docs/compressed-records.md).
Next: typed condition operands and subject dependencies, then the first canonical
script event/runtime state slice. Retail checksum handling is still unmeasured;
keep diagnostic extraction separate from runtime admission.

Checkpoint 27 adds typed winning condition words, VATS selector unions and static
form dependency bindings. Short layouts, unused words, tombstones and runtime
subject/value requests remain explicit; see [condition dependencies](docs/condition-dependencies.md).
Next: canonical script-instance/event state and version-bound persistence, then
primitive queries and observable execution. Unknown runtime defaults and native
behavior must remain unresolved instead of becoming successful no-ops.

Checkpoint 28 introduces a presentation-independent runtime crate with persistent
script/reference identities, exact numeric storage, typed local banks and pending
event contexts. All winning compiled schemas agree with an independent original
source reader; native snapshots preserve explicit engineering inputs. See
[script runtime state](docs/script-runtime-state.md).
Next: filesystem save publication/recovery, live foreign context resolution and
primitive query/execution slices. No original live values or defaults were captured.

Checkpoint 29 adds native script-state save repositories with campaign identity,
checked state revisions, owned worker captures, versioned checksummed chunks and
explicit previous-slot recovery. Cold processes restore the exact state; real
writer termination covers five publication stages. See [native saves](docs/native-saves.md).
Windows power-loss durability, full mutable world components, Bethesda save import
and NVSE cosaves remain open. Next: resolve foreign locals through explicit live
quest/placed event lists, then primitive queries and observable execution slices.

Checkpoint 30 resolves foreign locals through explicitly installed live owner
instances and supports typed atomic writes. All 25,549 compiled requests retain
their source identity and agree after same-process and cold native restoration.
The header classification, static attachment and operand comparisons are separate
from those engineering inputs; see [foreign live context](docs/foreign-live-context.md).
Next: immutable item/inventory inputs and primitive query components, then bounded
observable execution. Original list lifecycle, defaults and gameplay remain open.

Checkpoint 31 adds immutable base inventories for winning CONT/NPC_/CREA records,
including exact CNTO/COED fields, ACBS template flags and TPLT dependencies. See
[base inventory](docs/base-inventory.md). Next: bounded source dependency planning
and explicit mutable item counts, followed by primitive queries and observable
execution. Source counts do not establish template/leveled expansion, initialization,
equipment, ownership enforcement or original count coercion.

Checkpoint 32 reads all authored leveled item/creature/NPC lists and builds bounded
inventory/template/list dependency graphs. Exact source words and duplicate edges
remain intact; independent algorithms agree on every original graph edge. Three
base-item schema-domain mismatches remain reported. See
[leveled lists](docs/leveled-lists.md). Next: explicit live item counts/instances
and shared primitive queries, with differential measurements before adopting
selection, inheritance, coercion or original default rules.

Checkpoint 33 adds explicit canonical item banks, persistent identities, exact
optional attributes and atomic split/transfer/removal/fact replacement. Derived
count indices and bounded traces survive same-process and cold native restoration.
See [item state](docs/item-runtime-state.md). Snapshot schema 3 has explicit raw
migrations from schemas 1/2 without inventing inventory initialization. Next:
explicit older native-envelope import, shared primitive query adapters and
source-bound admission, followed by original differential execution measurements.

Checkpoint 34 adds explicit schema-2 native-envelope import with retained read-only
inputs, metadata/state agreement and full source-bound validation before creating
a new repository. Exact prior state survives cold restoration; inventories stay
uninitialized. See [native migration](docs/native-save-migration.md). Next: validate
explicit item keys against immutable winning source headers, then shared primitive
query adapters and original differential execution measurements.

Checkpoint 35 adds source-bound item checks using the shared immutable header
index and explicit caller kind rules. Every supplied source role is checked before
canonical mutation, with missing/deleted/wrong-kind cases preserving state.
Eight runtime tests and installed-data/cold/native comparisons cover the API; see
[source item checks](docs/source-item-validation.md). Next: shared primitive query
interfaces, observed original return coercion and bounded script execution traces.
Body/asset residency and original inventory admission remain separate gates.

Checkpoint 36 adds shared GetItemCount source-entry routing over explicit host
state, with persistent request context, exact bounded count traces and explicit
unverified numeric returns. Eight runtime tests and cold-process comparisons
cover engineering behavior; all winning CTDA bindings/descriptors are rerun. See
[primitive queries](docs/primitive-queries.md). Next: original form-list count and
argument/return coercion measurements, then bounded observable VM execution.
Unverified subject defaults and condition comparisons must continue to fail or
remain unresolved rather than being presented as successful original behavior.

Checkpoint 37 protects current verified metadata from the initial census generator.
Historical reconstruction requires a new local staging directory and exclusive
file creation; five real publication regressions run in the standard checks.
See [report publication](docs/report-publication.md). Gameplay dependencies remain
unchanged: measured original count/list/coercion behavior, bounded VM execution,
complete actor/player/quest persistence and retail comparisons are still open.

Checkpoint 38 adds immutable source form lists, ordered member bindings and bounded
structural closure. All 608 winning original lists and 6,090 member occurrences
match an independent source reader in the development comparison; final proof is
recorded separately in the checkpoint receipt. Eight tests cover namespaces,
tombstones, malformed/tainted input, duplicates, cycles, closure and budgets.
See [form lists](docs/form-lists.md). Next: measured original list expansion,
argument/return coercion and observable VM execution. Loading authored members
does not initialize runtime script-added lists or accept GetItemCount expansion.

Checkpoint 39 adds bounded vanilla postfix plans over retained source tokens,
ordered children and contiguous semantic subtree ranges. Independent forward Rust
and backward C++ construction agree on 53,401 complete expressions and three
authored residual-stack findings across all 14,514 bodies. Strict loading rejects
those findings; diagnostic reports retain them and still return 1. The expanded
debug CLI now starts with an explicit inspector worker stack. See
[expression plans](docs/expression-plans.md). Next: measure original handling of
the three source expressions and numeric/short-circuit/native coercion, then
source-bound control flow and bounded observable execution. This structural
model does not establish retail effects or accept gameplay.

Checkpoint 40 adds bounded source delimiter plans and explicit raw distance
relations. Independent forward Rust and backward C++ pairing agree on all
14,514 authored bodies: 14,461 complete structures and 53 first unresolved-body
findings. See [control-flow structure](docs/control-flow-structure.md). Next:
associate complete plans with immutable winning script versions, keep unresolved
source capability failures explicit, then bounded observable execution over
measured semantic primitives. Original jump rules, event lifecycle, numeric
effects and native handlers remain unfinished.

Checkpoint 41 combines winning source handles with bounded delimiter/expression
plans and each unit's own encoded operand tables. The development comparison
checks all 80,627 units and prepares 14,420 compiled source structures, retaining
every control/expression/table finding. See
[source-bound script plans](docs/source-bound-script-plans.md). Next: bounded
runtime frames and explicit capability checks over source-bound plans, plus the
remaining actor/player/state components. Original semantic measurements,
live context readiness and observable VM effects remain separate open gates.

Checkpoint 42 gives runtime worlds borrowed or shared immutable source ownership.
Owned worlds can outlive their loaders and move to workers, while snapshot/native
save formats and independent campaign banks retain their existing contracts. See
[shared runtime sources](docs/shared-runtime-sources.md). Next: source-bound live
event preparation and bounded runtime frames with explicit semantic capabilities,
then actor/player state and presentation integration. Ownership does not admit
original initialization, scheduling or bytecode effects.
