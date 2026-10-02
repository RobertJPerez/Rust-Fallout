# Implementation checkpoint 04: materials, textures and GPU preview

The Rust pipeline now decodes material properties, follows authored texture paths
through the installation's archives, and renders individual real models with Bevy.
The chair and Vit-o-matic cabinet have successful, visually inspected GPU captures.
The preview uses unlit diffuse materials; full interior assembly, retail shaders,
collision and gameplay remain unfinished.

| Check | Result |
| --- | --- |
| CLI build | Release, Rust 1.99.0 / Windows MSVC, passed |
| Bevy build | Pinned 0.19.1, debug model preview, passed |
| Workspace checks | Formatting, 47 tests, Clippy with warnings denied, passed |
| Independent payload comparison | 218 files; 751 objects; 403 meshes; 1,127 material blocks, all equal |
| Material comparison | Source f32 bits, flags, references, version fields, texture slots and path bytes agree |
| Deliberate comparison defects | Seven cases rejected, including changed material alpha/path and missing projection |
| House texture closure for decoded fields | 207 models; 1,120 material blocks; 854 references; 312 unique textures |
| Texture cache | 58,415,256 decoded bytes; all payload hashes verified; zero unresolved or ambiguous house paths |
| Full archive scan | 176,982 material blocks; 139,266 texture references in 25,729 successful scene files |
| Unsafe authored paths | Seven absolute exporter paths in six DLC models; retained and unresolved |
| Existing source failures | Same six legacy containers and 25 strict geometry rejections |
| Original archive identity | All 21 archive hashes still match the initial baseline |
| GPU smoke fixtures | Chair: 1 mesh / 316 triangles; Vit-o-matic: 28 meshes / 5,320 triangles |

## Implementation

`fallout-data::nif_scene::material` reads nine property/texture types without depending
on Bevy. Version-specific fields stay optional; unknown bits, source paths and texture
slots stay intact. References to texture sets/source images require the expected block
kind. Existing bounds, finite-number checks and array budgets also cover these fields
and derived path storage.

`ArchiveAssets` keeps immutable archive handles and a candidate index. It refuses
ambiguous mounts, caches archive digests during those handles' lifetimes, and exposes
unique reads to the texture probe and preview. `nif-assets` deduplicates texture paths,
records all usages, and publishes through the existing content-addressed cache. A bad
cache destination now fails before the batch instead of producing an error for each asset.

`fallout-preview` owns presentation-only meshes, DDS loading, material approximation
and an orbit camera. It retains source units, rotates source Z-up into Bevy Y-up,
preserves UVs, and checks DDS dimensions/mip payload lengths before upload. It skips
skinned meshes, reports unsupported blocks and uses magenta for unsupported diffuse
selection. The cabinet report includes one such binding; a successful capture does
not establish that every surface is correct. Keyboard bindings are implemented but
were not independently exercised by keyboard automation.

The actual captures are in ignored `local/preview-final/`. Its manifest binds both
PNG/report pairs to the tested executable. [model-preview.json](model-preview.json)
records hashes, geometry, limitations and the visual review. No proprietary model,
texture or screenshot is added to the distributable source tree.

## Evidence and limits

[nif-materials.json](nif-materials.json) records the corpus scan, exact sample
comparisons, source identities and all seven bad-path diagnostics.
[Material coverage](../parity/nif-material-coverage.json) separates independently
compared types from `NiShadeProperty`, which has synthetic coverage only. Exact
independent comparisons cover 218 files, not the entire corpus.

The separate GPL nifly oracle gained a raw material projection. It still bypasses
automatic preparation and remains outside the Rust runtime. No schema-generated code
or upstream C++ implementation was copied into the decoder. Bevy's exact crate version,
source revision and dependency declarations are recorded in the source lock and license
inventory; this is not a completed transitive per-file audit.

There are no accepted gameplay scenarios. The base LAND checksum issue, two malformed
Misc entries, 25 geometry failures, six legacy NIF containers, archive/loose precedence,
retail effective-profile capture, scripts, saving and other-game adapters remain open.
The seven exporter paths also need evidence before any compatibility repair is chosen.

Run commands and inspection limits are in [model preview](../docs/model-preview.md)
and [material decoding](../docs/nif-materials.md). The next rendering gate is a complete
real interior with verified placement transforms, materials, collision and movement,
followed by Goodsprings. The broader brief remains in [NEXT_STEPS.md](../NEXT_STEPS.md).

Prior evidence is preserved in [checkpoint 03](checkpoint-03.md), including its source
snapshot and verification report. Current identities are in
[source-snapshot.json](source-snapshot.json) and [verification.json](verification.json).
The working tree is uncommitted; no source digest is represented as a Git revision.
