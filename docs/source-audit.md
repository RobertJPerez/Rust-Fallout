# Source audit, 2026-10-02

The complete 2,059-line master brief was read and copied without alteration. Its SHA-256
is `540f544a41d76388e30f80af939e045f95e2a6310e4df2b047e60cd4e4efc76e`.
[sources.lock.json](../sources.lock.json) catalogs 70 canonical GitHub repositories from
that brief and records resolved commits. Pinning repository metadata does **not** mean
every repository was read, built, or audited. The inspected paths and actual build
results are recorded individually; the remaining leads are explicitly deferred.

The local research checkouts live under ignored `.research/`. No commercial Gamebryo
source was ingested, and this work is not described as a clean-room implementation.

| Project and pin | Inspected evidence / actual use | Result and limit |
| --- | --- | --- |
| [dream_archive](https://github.com/DreamWeave-MP/dream_archive/tree/a6ba4c9416e5c7bc211e7a3762420e5bf5c9c049) | Rust manifest, MIT notice, BSA implementation and inflation paths; sole runtime archive dependency | Built with BSA TES4-family feature only. All NV v104 indexes compared; 182,175 byte-equal payloads and two strict decode failures. Other advertised archive families are untested here. |
| [bsa-rs / ba2](https://github.com/Ryan-rsm-McKenzie/bsa-rs/tree/7b90ce145d5de8e1dda280f10b2b0e0b5aa067f9) | Rust implementation/manifest and actual 0BSD license; offline archive oracle | Built against the same corpus. Pulls native DirectXTex/compression dependencies, which are not linked into the runtime. Accepts the two malformed text entries. |
| [esplugin](https://github.com/Ortham/esplugin/tree/b33bd81aae02dd5cc797769705b093e407cd4a66) | Rust framing/metadata/identity implementation and GPL notices; independent executable | Built in a separate GPL-3.0-or-later workspace. All ten record counts, declared totals, and master lists agree. Its parser does not validate compressed record bodies. No implementation copied into our runtime. |
| [xEdit](https://github.com/TES5Edit/TES5Edit/tree/9fb016884bec138ea6c7b872cec831537d464c3e) | MPL-2.0 notices; FNV header, script metadata, CTDA, player binding, and file-ID mapping definitions | Source reference only. Delphi application and independent per-record exports not run. Field layouts are not evidence of gameplay execution semantics. |
| [Bevy v0.19.1](https://github.com/bevyengine/bevy/tree/b56fc29d3016e641754765244b5ba3f9cc504671) | Pinned manifest, custom-mesh example, headless capture, image/DDS and sampler APIs; MIT/Apache declarations | Linked in separate `fallout-preview`; debug build and two real-model GPU captures work. Unlit diffuse inspection only; no retail lighting, shader or physics parity. |
| [ByroRedux](https://github.com/matiaszanolli/ByroRedux/tree/273692be5638bd9c20f63ec022d659d1d832956e) | README/provenance review | Deferred. README claims MIT but no root LICENSE was found in the inspected checkout; component notices/provenance need work. README points to commercial engine source. No code copied, linked, or built. Advertised runtime breadth is not verified. |
| [xNVSE](https://github.com/xNVSE/NVSE/tree/0ccd23ad885ddae533c1790a3fc56cd073e38de3) | Selected GameScript declarations for future command research | Per-file license audit incomplete; not built or reused. Wrappers around original executable addresses do not supply standalone implementations. |
| [nifxml](https://github.com/niftools/nifxml/tree/970a6238218a106daaeb89a61bcda0eeaf9d08c4) | GPL-3.0 root license; container, scene inheritance, geometry/topology and selected property layout facts | Format reference only. No XML schema or generated parser copied/linked into the runtime. Six scene/mesh and nine material/texture payload types independently implemented in Rust; no shader behavior acceptance. |
| [nifly](https://github.com/ousnius/nifly/tree/cca0a770094bb962fb28ea1fec5ea903e68fda8e) | GPL-3.0 library; header/factory/geometry/object/transform APIs; raw block loading; half MIT and Miniball GPL notices | Separate MSVC oracle: 218 containers / 4,582 block entries match; raw supported payloads also match for 751 objects, 403 meshes and 1,127 material blocks. Raw diagnostics confirm all 25 rejected model inputs. No rendering/animation/physics parity or file writes/conversions. |
| [OpenMW](https://github.com/OpenMW/openmw/tree/63f6261b6e1fe1eb6170ad4e686de0edec836114) | GPL-3.0 root license; selected placement conversion, world/scene and NIF loader paths | Source reference only, no build/copy/link. Consulted static rotation order and declared marker names/flag. Original Rust math has analytic tests; this convention is provisional until measured NV retail comparisons. |

Checkpoint 06 also consults the pinned nifxml alpha/stencil/shader bit fields and
OpenMW's NoLighting loader/vertex/fragment paths for empty-texture and falloff facts.
Our original Rust/WGSL inspection adapter now supports untextured bindings and
alpha/culling/depth states. The pinned Bevy extended-material API hosts those states;
62 synthetic GPU checks verify their numeric output. No OpenMW C++/GLSL implementation
was copied or linked. Falloff, emittance and retail lighting remain unimplemented.

Exact inspected path lists, build commands, language, license-file locations, and
test-data provenance accompany those entries in the lock file. A candidate's README
claims remain claims. The other unaudited atlas entries remain research leads at pinned
revisions. Checkpoint 05 adds exact BSXFlags field comparisons on all 218 prior sample
files and two real-interior GPU views. The selected OpenMW paths informed presentation
research; no upstream runtime behavior is claimed as accepted Fallout compatibility.

Checkpoint 07 reads nifxml collision inheritance and the pre-Skyrim body, primitive,
wrapper and packed-triangle layouts. Nifly's bhk.hpp/bhk.cpp supply an independent
raw oracle projection, including stored float bits and CNG source/block hashes.
The house's 731 supported collision blocks match exactly across 207 files. New
runtime parsing, shape graph validation and comparison/evidence tooling are original
Rust. The GPL C++ oracle remains a separate executable. No upstream runtime code was
copied into Rust, and parsed collision data is not physics acceptance. See
[collision decoding](nif-collisions.md).

Checkpoint 08 adds original Rust plugin-index serialization, verified cache lookup,
source hashing and killed-process publication tests. No new runtime dependency or
upstream parser implementation is introduced. The source layouts and resolver rules
remain those already pinned and tested; the new evidence compares every selected
cell field with the uncached path. The earlier collision and GPU oracle results
retain their original checkpoint/source identities.

Checkpoint 09 consults the pinned xEdit WRLD/LAND definitions, shared landscape
layouts and VTXT position checks. Original Rust decoders retain source fields and
unknown bytes. A separately authored C++ field oracle uses only the standard library
and Windows CNG; it has no upstream parser dependency and is not linked into Rust.
Its actual build/hash and selected field comparisons are recorded separately from
xEdit's unexecuted Delphi application and from retail behavior. Decompression and
canonical resolution are outside that field oracle's scope.

Checkpoint 10 consults the pinned OpenMW ESM4 height conversion, LAND dimensions
and source-grid indexing. The original Rust converter uses a linear accumulator;
the original C++ oracle independently evaluates each sample's dependency path.
No OpenMW source is copied or linked and no upstream application is run. Root and
per-file license facts and inspected hashes are in the source lock. Exact height
comparisons certify that reference model only, not measured NV terrain behavior.

## Reproduce the component builds

Checkpoint 13 consults contiguous alpha-layer indices and byte coverage in the
already pinned OpenMW terrain files. Original Rust and C++ calculations expand
quadrant-local grids, preserving missing/default materials and reporting excess
coverage. No upstream blend implementation was copied or linked. This comparison
certifies the inspection model, not executed OpenMW or retail NV rendering.

Checkpoint 12 consults the pinned xEdit LTEX/TXST and NULL LAND definitions, plus
the already pinned OpenMW texture-root/default handling as source references.
Original Rust resolves authored nonnull chains; original C++ projects exact fields.
The separate ba2 member oracle independently reads selected archive bytes, sharing
file guards and SHA helpers but no runtime archive parsing. No upstream application
was run and no implementation was copied into Rust. NULL defaults and texture/blend
behavior remain unaccepted. Updated notes and source hashes are in the lock file.

Checkpoint 11 consults pinned OpenMW checkerboard diagonals and signed normal
storage, and verifies xEdit's XCLC land flag byte plus three unused bytes. Original
Rust/C++ geometry is compared exactly; the separate Bevy adapter renders source
terrain with authored colors. No upstream implementation is copied/linked or
application executed. Inspection winding/color space and absent-color presentation
are not measured retail behavior. Source hashes and limits remain in the lock file.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\cargo.ps1 build --release --locked -p fallout-cli
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\cargo.ps1 build --release --locked -p archive-compare
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\cargo.ps1 build --release --locked --manifest-path tools\plugin-oracle\Cargo.toml
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\compare-corpus.ps1
```

All comparison data came from the authorized local Steam installation. Unit/integration
fixtures were written for this project. No retail game payloads or upstream test assets
are included in distributable fixtures. Local extracted assets remain under ignored
`local/cache/`.

## Notices and open audit work

The actual notices read for the directly used archive/oracle components are retained
under [licenses](licenses/). [dependency-licenses.json](../reports/dependency-licenses.json)
records Cargo manifest license declarations for the resolved workspace graph, including
tooling dependencies. This inventory is not a completed per-file audit of every
transitive dependency, and it excludes the separately locked GPL plugin oracle and
the separate CMake nifly oracle. Nifly's pin and bundled-library notices are recorded
in the source lock and `docs/licenses/nifly-*` files.

The new project code has no outbound distribution license selected yet. Cargo packages
are private (`publish = false`); do not infer an upstream project's license applies to
this project. Keep upstream notices with any eventual distribution of their components.

Known primary-source format exception reports are linked in
[format-exceptions.md](format-exceptions.md). New code and source research should update
the existing pins and evidence, rather than replacing unverified entries with optimistic
support labels.
