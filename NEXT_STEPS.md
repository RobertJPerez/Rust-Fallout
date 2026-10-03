# Resume here

The next unmet gate is **M1: trustworthy, semantically useful content loading**.
The current tools build and run; the original games have not been recreated yet.
Keep the entire master brief in scope and progress through its dependency gates.

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
   a canonical content store, WRLD/terrain and remaining fields,
   full asset dependency closure, and record-specific override rules.

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
   There are 14,514 compiled bodies and 148 observed condition IDs; execution is absent.

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
