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
| [xNVSE](https://github.com/xNVSE/NVSE/tree/0ccd23ad885ddae533c1790a3fc56cd073e38de3) | Complete ScriptAnalyzer.h/.cpp; line/event framing and statement IDs; CommandTable::Init ranges; common/Algohol notices | Per-file license audit incomplete; not built or reused. Wrappers around original executable addresses do not supply standalone implementations. |
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

Checkpoint 14 uses the original quadrant weights in an original Rust diffuse
inspection adapter and Rust test launcher. Existing bounded DDS/material code is
reused; no upstream terrain shader is copied or linked. Independent source/byte
checks and 64 original GPU expectations certify the stated inspection model,
while retail blending, tiling, visibility and lighting remain unaccepted.

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

Checkpoint 15 reads the pinned xNVSE ScriptAnalyzer files completely and uses their
line/event layouts as format evidence. Its per-file licensing is not established
by the separate common/Algohol notices. Original Rust owns the bounded decoder;
an original offline C++ tool compares Rust-extracted SCDA headers and CNG hashes.
Neither upstream implementation is copied or linked, and no engine address is
used. See [compiled script inspection](compiled-scripts.md) for scope and remaining
operand/execution work.

Checkpoint 16 adds source-bound, offline descriptor inspection of the original
executable. Pinned xNVSE table locations are translated through bounded PE32
sections; no original code is loaded or called. An original C++ reader independently
reads the same executable and compares names, parameter metadata and provenance.
Microsoft PE documentation is the format reference. All command behavior, return
types and event scheduling remain open; see [command catalogue](command-catalogue.md).

Checkpoint 17 inspects xEdit script table definitions and the xNVSE reference/local
lookup functions. The original Rust decoder retains byte-level field provenance,
source-less tables and all authored duplicate declarations. An original offline
C++ tool parses complete Rust-extracted decoded records independently, including
extended fields and top-level caller associations. Three empty-unit count
inconsistencies remain explicit. The comparison does not certify plugin extraction,
stable identity conversion, retail loading or script behavior. See
[script table bindings](script-bindings.md).

Checkpoint 18 rereads the vanilla expression routines and operator declarations.
An original Rust tokenizer preserves decimal lexemes, quoted bytes, context
prefixes and opaque command arguments. Original C++ independently reads the
operator table and parses Rust-extracted SCDA; all envelopes and token digests
agree. Shared offline C++ PE helpers remain separate from Rust runtime code.
Microsoft CRT documentation identifies historical parsing differences; current
CRT boundary comparisons do not certify original values, rounding or evaluation.
See [compiled expressions](script-expressions.md).

Checkpoint 19 inspects native extraction classifications and message argument
structure. Original Rust and C++ readers preserve typed bits and trailing hashes;
all 67,931 authored instruction/expression calls agree. The shared offline
expression reader and descriptor catalogue are freshly compared after refactoring.
ShowMessage trailing words retain unverified semantics. Native type checks,
conversions and behavior are unfinished; see [native operands](native-arguments.md).

Checkpoint 20 associates compiled indices with their owning script tables.
Original C++ independently parses complete decoded records and compares every
association digest. Foreign local declarations remain deferred to loaded target
scripts; no runtime value or form-existence claim is made. Shared original
offline table/native helpers retain complete fresh regression comparisons. See
[operand associations](operand-bindings.md).

Checkpoint 21 reads xEdit's CTDA optional layouts and selected common comparison
helpers. Original Rust and C++ compare every authored condition field and source
extent. Short layouts and two records' unexplained fields remain intact; no editor
migration or condition evaluation is applied. Function parameter meanings and
record ownership/grouping remain open. See [condition fields](condition-fields.md).

Checkpoint 22 reads the pinned xEdit QUST, INFO, DIAL and embedded-script schema
blocks completely, plus the selected common NextSpeaker, QSTR and editor-only
INFO-order definitions. Original Rust retains borrowed fields, ordered section
identities and separate condition/script owners. Original offline C++ compares
all source fields and owners. Two short quest headers and eighteen orphaned shared
infos are retained; xEdit's after-load cleanup and default insertion are not applied.
This is schema evidence, not verified retail selection or timing. See
[narrative structure](narrative-structure.md).

Checkpoint 23 reads the selected xEdit group-label formatting implementation:
group 7 is topic children and its label is a source FormID. Original offline C++
now independently reads original plugin headers, master tables and parent group
contexts, rebuilds canonical whole-record winners and compares all metadata
and winning INFO memberships. Its write-denying source handles stay open during
inspection. Payloads other than TES4 remain deferred; retail INFO ordering and
record-specific override behavior remain open. Index cache format v2 has fresh
cold/warm, corruption and selected-cell regression evidence. See
[dialogue membership](dialogue-membership.md).

Checkpoint 24 uses the already inspected xEdit ownership/table definitions and
xNVSE first-variable lookup source. Original Rust loads immutable winning script
definitions and retains source versions, authored declarations and reference
dependency states. Original offline C++ rebuilds original headers and namespaces,
compares all loaded unit metadata and versions, and independently filters the
bound full checkpoint 17 table scan for completeness. Uncompressed bodies are
compared directly with original bytes; compressed extraction remains Rust-owned.
Moving the original header reader into a shared offline helper triggers a fresh
complete checkpoint 23 regression. No live event lists, runtime values or script
execution are implemented. See [loaded scripts](loaded-scripts.md).

Checkpoint 25 reads the pinned xNVSE GetReferencedScript, GetVariableInfo,
ResolveExternalVar, EventListFromForm, GetParentScript and GetVariableName source
sections, plus the full xEdit QUST schema block. Original Rust follows authored
quest SCRI attachments and associates foreign declarations. Placed-reference
ExtraScript/event-list selection stays explicit and deferred; base scripts are
not substituted. Original offline C++ independently compares every winning quest
attachment and compiled foreign operand. Refactoring the original operand helper
triggers a fresh full operand/table/native/expression/descriptor regression;
shared Rust inspection inputs trigger a fresh full loaded/header/cache comparison.
No runtime values, scheduling or quest behavior are certified. See
[quest script attachments](quest-script-attachments.md).

Checkpoint 26 reads both complete RFC 1950 and RFC 1951 texts, including notices.
The original offline C++ compression reader implements their format rules without
copying sample code or linking an upstream zlib implementation. It directly reads
all original compressed record bytes and independently compares stored/decoded
hashes and checksum findings. Authored edge frames and deterministic Rust-library
frames exercise all block kinds, backreferences and malformed boundaries. The
existing original header reader supplies source namespaces and metadata; its
implementation is unchanged in this checkpoint. Strict production checksum policy
is unchanged. See [compressed records](compressed-records.md).

Checkpoint 27 reads the complete pinned xEdit condition function table, parameter
type declarations, parameter unions, VATS selector union, variable-name lookup,
run-on/reference deciders and legacy target after-load migration. It also reads
xNVSE's parameter enum and in-memory Condition structure. Original CTDA word
classification is separate from native argument extraction. An original offline
C++ reader now extracts winning condition payloads directly, using the unchanged
checkpoint 26 RFC decoder for compressed bytes. It independently compares every
typed word, source namespace, static dependency and binding digest. No upstream
condition implementation is imported; editor migration, runtime target-kind
acceptance, live values and query evaluation stay unimplemented. See
[condition dependencies](condition-dependencies.md).

Checkpoint 28 reads selected pinned xNVSE ScriptLocal/ScriptEventList definitions,
compiled declaration classification, reference extraction, copy/reset and
external event-list selection sections. The reviewed constructor wraps opaque
original code, so initialization remains unresolved. Original Rust stores explicit
host values in typed canonical state; an original offline C++ reader directly
extracts all winning compiled schemas using unchanged source/compression helpers.
Every first declaration and classification agrees. Storage, identity, event
journaling and snapshot restoration have engineering proofs, not retail behavior
acceptance. No upstream implementation is copied. See
[script runtime state](script-runtime-state.md) for exact reference scopes.

Checkpoint 29 reviews Rust 1.99 file sync, rename and nonblocking lock contracts,
alongside the Windows MoveFileExW and FlushFileBuffers documentation. Original
Rust owns the versioned native save container, worker capture and repository
publication/recovery code. An original offline C++ reader independently validates
container extents, metadata and SHA-256 values; it does not interpret the JSON
state. No upstream persistence implementation is copied or linked. Process-kill
tests run on Windows NTFS; directory/power-loss durability remains unverified.

The complete pinned getrandom 0.4.3 Cargo.toml.orig, LICENSE-MIT, LICENSE-APACHE,
src/lib.rs and src/backends/windows.rs were read from the local Cargo registry.
This existing resolved dependency is now a direct runtime dependency for OS random
campaign identities. It is linked unchanged under its MIT-or-Apache-2.0 terms;
the reviewed Windows backend uses ProcessPrng and failures propagate. Other
platform backends were not audited or tested in this checkpoint. See
[native saves](native-saves.md) for the contracts and primary documentation links.

Checkpoint 30 rereads selected pinned xNVSE foreign-variable reference resolution,
quest/placed event-list selection and parent-script lookup, plus GameForms.h lines
1-140. Original Rust resolves explicit host instance banks and preserves typed
values; authored SCRI and base links never supply missing live values. Original
offline C++ rebuilds all winning header classifications. Fresh static quest,
operand, descriptor, loaded/header/cache and native-save/schema regressions bind
the new requests to independently checked source facts. No upstream implementation
is copied or linked and no original live state is captured. Exact selected scopes
and file hashes are recorded in the source lock; see
[foreign live context](foreign-live-context.md).

Checkpoint 31 reads selected pinned xEdit CNTO/COED, CONT, actor ACBS/TPLT and
inventory template declarations, plus the complete common COED owner decider.
It reads selected xNVSE TESContainer/ContainerExtraData and ExtraContainerChanges
layouts, and the complete GetCountForForm helper. Disk condition bits are 32-bit;
the runtime layout uses a double. Neither conversion nor query behavior is assumed.
Original Rust and offline C++ independently retain exact source fields and bind
master-relative links. No upstream implementation is copied or linked. Line scopes
and whole-file hashes are recorded in sources.lock.json; see
[base inventory](base-inventory.md).

Checkpoint 32 reads the complete selected pinned xEdit LVLI/LVLC/LVLN definitions
and common leveled-entry helper, plus the selected xNVSE leveled-list load/runtime
layouts. Disk level/count words and optional field absence stay exact; editor
defaults and original signed runtime conversion are not applied. Original Rust
uses iterative Kosaraju and original C++ uses iterative Tarjan for bounded source
graphs. Full winning list fields, inventory/template links and SCCs are compared;
no upstream algorithm implementation is imported and no random selection occurs.
See [leveled lists](leveled-lists.md) and source-lock scopes for exact provenance.

Checkpoint 33 reads selected pinned xNVSE extra-data declarations for script, health,
worn, count, ammo, ownership/rank/global and weapon modification storage. Exact
32/64-bit condition widths stay distinct; signed count conversion and opaque words
are not assigned original semantics. Original Rust owns explicit host transactions,
indices and persistence. Source identities and container integrity have independent
comparisons; mutable values and query sums have engineering tests only. No upstream
implementation is imported. See [item state](item-runtime-state.md) for scopes.

Checkpoint 34 extends the original native-container implementation with explicit
schema-2 import. It reuses the previously documented file/checksum contracts and
strict raw-state migration; no new upstream implementation is imported. Independent
C++ requires an explicit schema-2 flag and checks source/target container integrity
only. Full prior-state preservation and source-bound restoration are engineering
proofs; Bethesda saves remain separate. See [native migration](native-save-migration.md).

Checkpoint 35 shares the existing independently checked winning header index
with item source validation. Callers explicitly declare kind rules for each role;
no original admission domain is guessed. Original Rust validates links before
canonical transactions. Whole-header classification and exact source binding
comparisons are separate from host-policy engineering tests. No new upstream
implementation is imported; see [source item checks](source-item-validation.md).

Checkpoint 36 reads the pinned xNVSE inventory-object-or-form-list parameter
declaration and xEdit GetItemCount condition row. The exact original executable
supplies command 4143/function 47, required parent and parameter type 50. Both
source entry IDs share an original Rust host query; no original handler executes
and argument/return coercion remains unverified. Independent full descriptor/CTDA
comparisons stay separate from engineering result tests. Exact scopes/hashes are
in the source lock; see [primitive queries](primitive-queries.md).

Checkpoint 38 reads the selected pinned xEdit FLST definition and complete
form-list editor sorting helper, plus the selected xNVSE BGSListForm declaration.
Physical authored LNAM order is retained; editor sorting and live script-added
member accounting are not imported as runtime policies. Own Rust/C++ projections
read original source independently and compare every member binding and structural
edge. Shared Rust Kosaraju and independent C++ Tarjan remain iterative and bounded.
No upstream implementation is copied or linked. See [form lists](form-lists.md)
and source-lock ranges/hashes. GetItemCount expansion and retail execution stay open.

Checkpoint 39 reinspects selected ScriptAnalyzer operand arity/order, prefix
consumption and residual-stack diagnostics, plus GameScript operator declarations.
Own Rust constructs a flat plan with a forward operand stack; the independently
owned C++ tool scans backward with pending child slots. Exact executable descriptor
identities constrain the model. All source expression/body hashes and structural
findings agree in the development corpus comparison, including three residual
stacks which remain blocked pending retail measurements. No upstream code is
copied or linked; selected scopes/full-file hashes are retained in the source lock.
Source extraction is inherited from the independently checked existing pipeline;
the new C++ tool does not independently extract plugins or execute scripts.
See [expression plans](expression-plans.md). Numeric effects and VM acceptance
remain unfinished.

Checkpoint 40 reads selected pinned ScriptAnalyzer framing, begin/conditional
field readers, statement dispatch and matching declarations. Raw field names
are retained as reference facts; observed instruction-count relations are
explicitly separated from original jump semantics. Own Rust pairs delimiters
forward and own C++ pairs them backward after a structural audit. Every original
body and first unresolved finding agrees in the development comparison. The
new oracle consumes the existing independently checked extraction bundle; it
does not independently extract plugins or execute scripts. No upstream code is
copied or linked. See [control-flow structure](control-flow-structure.md) and
the exact source-lock scopes. All original execution acceptance remains open.
