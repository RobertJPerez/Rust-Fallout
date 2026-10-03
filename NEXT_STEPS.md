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
