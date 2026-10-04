# NV skin source decoding

ASSET-01 decodes `NiSkinInstance`, `BSDismemberSkinInstance` and `NiSkinData`.
It preserves authored links, order, counts, dismemberment words and finite binary32
bits. It does not evaluate skinning or decode `NiSkinPartition` payloads.

## Source contract

The admitted container tuple is file `20.2.0.7`, user `11`, little endian, and
Bethesda stream `14,21,24,25,26,27,28,30,31,32,33,34`, exactly as admitted by the
existing NV container reader. These three skin layouts have no stream-dependent
fields within that set. Other tuples use no fallback.

The independently consulted sources are
[nifxml](https://github.com/niftools/nifxml/blob/970a6238218a106daaeb89a61bcda0eeaf9d08c4/nif.xml)
`970a6238218a106daaeb89a61bcda0eeaf9d08c4` (`nif.xml`: `NiObject`, `NiTransform`,
`Matrix33`, `NiBound`, `BoneData`, `BoneVertData`, the three skin blocks and
`BodyPartList`/`BSPartFlag`/`BSDismemberBodyPartType`) and
[nifly](https://github.com/ousnius/nifly/blob/cca0a770094bb962fb28ea1fec5ea903e68fda8e/src/Skin.cpp)
`cca0a770094bb962fb28ea1fec5ea903e68fda8e` (`include/Skin.hpp` and `src/Skin.cpp`,
read completely). Their GPL sources supply format facts; no schema generation or
upstream parser source is copied into Rust. The native oracle is an isolated GPL
executable, never linked into the replacement runtime.

For this tuple:

- `NiSkinInstance`: data ref, partition ref, skeleton-root pointer, uint32 bone
  count, and ordered bone pointers. The partition ref is present from
  `10.1.0.101`; there are no inherited `NiObject` payload fields.
- `BSDismemberSkinInstance`: the instance fields, uint32 partition count, then
  ushort flags and ushort body-part ID for every authored entry. Unknown words
  remain exact; the decoder does not supply defaults or interpret combat.
- `NiSkinData`: rotation (nine floats in source order), translation (three),
  scale, uint32 bone count, raw weight-presence byte, then the bones. The old
  data-level partition ref occurs only in `4.0.0.2` through `10.1.0.0` and is
  absent here. The presence byte exists from `4.2.1.0`.
- Each bone: the same 52-byte transform, 16-byte sphere, raw ushort vertex
  count, and six-byte index/weight pairs only when the presence byte is nonzero
  (`BoneData`'s applicable branch is `since 4.2.2.0`). Nonzero presence bytes
  are retained, including values above one. A zero byte can accompany a nonzero
  raw count without weight pairs; the source count must still be retained.

The pinned nifly reader normalizes presence bytes above one and zeroes the bone
vertex count when weights are absent. Its reconstructed fields cannot certify
either raw field. The dedicated oracle must supplement factory output with
independent reads of these source fields and report normalization explicitly.

## Decoder boundary

The proposed public entrypoints are `nif_skin::decode` and `decode_with_limits`,
returning the existing `NifIndex` and a skin source catalogue. The existing scene
decoder supplies immutable geometry-owner links and vertex counts; its strict
float/index and graph checks remain prerequisites. It is not modified. Scene
array storage and skin catalogue storage have separate explicit limits, while
each input and block table retain their established limits. A separate default
16-million-check budget bounds repeated weight-index validation for shared owners.

Nonnull links require their source target kinds. Null links remain explicit
dependencies. Bone counts must match each linked data block. Every weight index
is checked against every decoded geometry owner, including shared skins and
shared data. A skin without decoded owners cannot certify vertex bounds.
Undecoded node subclasses and partition payloads remain dependencies. Their
block names do not establish skeleton evaluation or hardware-skinning support.

Malformed sizes/counts, truncation, surplus bytes, wrong-kind/out-of-range links,
bone-count mismatches, nonfinite floats, negative radii, invalid weighted vertex
indices and budget exhaustion fail with source context. Finite weights, including
negative/over-one/duplicate entries, are preserved without normalization, pruning
or automatic repair. Matrix storage is preserved; no transform is composed.

## Development verification

The first ASSET-01 run is retained in the asset worktree at
`local/asset-01-dev-20261003-01`. It is development evidence, not a numbered
integration checkpoint or whole-game acceptance receipt.

| Executed check | Result |
| --- | --- |
| Skin tests | 19 pass, including all admitted streams, exact bits/raw fields, malformed input, sharing and both storage/work budgets |
| Existing NIF/container/scene/collision tests plus skin | 52 pass |
| CLI tests | 12 pass |
| Formatting and data/CLI Clippy, all targets, warnings denied | Pass |
| Dedicated MSVC raw-factory oracle | Built in private `local/nif-skin-oracle-build`, parallelism 2 |
| Authored native/Rust comparisons | 48 exact files: all 12 admitted streams and presence bytes 0, 1, 2, 255 |
| Normalization supplements exercised | 24 presence-byte and 24 absent-weight vertex-count cases |
| Deliberately altered comparison reports | All 24 rejected, including source/binary provenance, missing/duplicate files, links/order, integer words, float bits, spans and owner fields |
| Bounded original source sample | 32 files from nine archives; 23,261,738 bytes decoded during selection; zero selection findings |
| Original source fields | 500 blocks, 250 owner associations, 2,121 bone transforms and 270,536 weights agree exactly |
| Remaining sampled dependencies | 250 unresolved `NiSkinPartition` payloads |

The sampled block totals are 250 `NiSkinData`, 248
`BSDismemberSkinInstance` and two `NiSkinInstance`. All original samples have
stream revision 34; older admitted revisions have authored coverage only in this
slice. They cover four character, ten creature and eighteen equipment files.
No first-person skin was selected within the bounded candidate search. Gun
Runners' Arsenal was examined but yielded no sampled skin. This selection is not
complete corpus or actor/attachment coverage. Raw presence bytes above one and
absent-weight nonzero counts were exercised in authored inputs, not observed in
these 32 original samples.

The CLI exposes the source decoder through `fallout nif-skin INPUT`, optionally
with `--oracle-report REPORT`. It writes a report and returns nonzero on decoding
or comparison failures. Dependencies remain reported alongside exact source
success; `runtime_ready` remains false. The original archive sampler is an
explicitly ignored test unless supplied original inputs and a new local output
directory. Missing input is not a passing comparison.

Run from the assigned asset worktree:

```powershell
$env:CARGO_TARGET_DIR = 'G:\Rust-Fallout-worktrees\assets\target'
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked -p fallout-data --test nif_skin
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 clippy --locked -p fallout-data -p fallout-cli --all-targets -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-nif-skin-oracle.ps1
py -3 tools/nif-skin-oracle/check_comparison.py --output-dir local/new-authored-skin-run
```

For a new original-data sample, set `ASSET_SKIN_ARCHIVES` to a JSON array of
explicit archive paths and `ASSET_SKIN_EVIDENCE` to a new directory whose existing
parent is under this worktree's `local` directory. Run the ignored
`nif_skin_samples` test with `--ignored --nocapture`. It uses the existing
production archive reader, caps candidate checks per category, selects at most
64 skins and caps all decoded scan input at 64 MiB. Then capture the native
projection and compare using this worktree's binary:

```powershell
py -3 tools/run-nif-skin-oracle.py --input local/new-sample/inputs --output local/new-sample/oracle.json
target\debug\fallout.exe nif-skin local/new-sample/inputs --oracle-report local/new-sample/oracle.json --output local/new-sample/rust.json
```

The source manifest retains archive path, entry index, original member path
bytes, exact decoded hash/length, tuple and container block counts. It preserves
selection failures. It does not rehash whole installation archives or certify
unchanged installation state; final source-baseline checks belong to integration.
Research checkouts stayed clean. Raw source assets remain ignored and local.

## Integration and remaining gates

Minimal shared wiring is one `pub mod nif_skin` export in fallout-data and the
module/command/dispatch additions in the CLI root. The primary agent owns their
integrated versions, the source lock, parity ledger and final checkpoint proof.
No scene/container/cursor, preview, runtime or save-contract file is edited.

Next: animation source framing (ASSET-04), then independently verified evaluation
contracts. Complete-source skin-root membership, evaluated poses, transforms composed for
skinning, normalization rules, dismemberment behavior, animation, attachments
and retail presentation/gameplay remain unverified.

## ASSET-02 partition source contract

This bounded slice preserves the ASSET-01 entrypoints. Its additive
`nif_skin::partition::{decode, decode_with_limits}` returns the existing index and
`Source { skin, partitions }`. Partition limits contain the existing skin limits,
a separate 128 MiB array-element budget and a 16-million-check aggregate budget
for repeated palette/map validation across all linked instances and owners.
The CLI/native `--include-partitions` mode uses source-projection schema 2; the
default ASSET-01 schema-1 path remains available.

For admitted file/user/Bethesda tuples, each partition starts with five ushort
counts (vertices, triangles, bones, strips, weights per vertex), an ordered ushort
bone palette, a map-presence byte and optional ushort vertex map, a
weight-presence byte and optional vertex/weight array, ushort strip lengths, a
face-presence byte and authored strip or triangle arrays, then a bone-index
presence byte and optional byte indices. No source arrays are regenerated.
`#BS_GT_FO3#` means stream greater than 34; `#BS_SSE#` means stream 100. Their
LOD/global-VB and SSE descriptor/vertex-buffer/triangle-copy fields are absent in
all admitted NV streams. The pinned nifly reader's corresponding gates are
user at least 12 and user at least 12 plus stream 100; these are also absent here.

The XML gives weights and bone indices the declared width. The pinned native
reader instead uses fixed four-element structures. The initial independently
compared branch therefore requires width four whenever nonempty weight or
bone-index arrays are present. Other nonempty widths remain explicitly
unsupported. Absent/zero-length arrays retain their authored width/count fields.
Presence flags must be source bytes zero or one in this branch; other values are
reported rather than normalized. Finite weights and authored duplicate/degenerate
topology remain exact. No weight normalization or strip triangulation is performed.

Local topology indices and byte bone indices have their declared domains checked.
Every palette is checked against every referring skin instance's ordered bone
array, and every map against every decoded geometry owner's vertex count. Missing
owners/maps or unresolved geometry remain explicit. Dismember/partition count
relationships are preserved and diagnosed; no automatic correspondence repair
is introduced. All parsing/count/storage/work limits retain source context.

Read-only preflight evidence is
`local/asset-02-dev-20261003-01/partition-preflight.json`. The 32 frozen ASSET-01
samples contain 250 partition blocks and 606 partitions: 554 strip partitions,
52 triangle partitions, 612,152 weight words, 153,038 mapped vertices and 6,473
palette entries. All sampled widths are four and all four presence flags are
one. The preflight found no palette/map/local-index/nonfinite/count discrepancy.
The preflight is read-only source research; the separate verification below
checks the implemented decoder against the pinned native factories.

The combined entrypoint retires `PartitionPayload` dependencies only after every
selected partition block has decoded successfully. Remaining skin dependencies
stay in `Source.skin.dependencies`; partition-specific diagnostics are in
`Source.partitions.dependencies`. Zero dependencies certifies neither scene
membership nor evaluated skinning. Both catalogues retain `runtime_ready: false`.
Absent-array diagnostics use a field mask: map 1, weights 2, faces 4, indices 8.
Each relation costs a work-budget unit even with empty arrays, so shared empty
partitions cannot bypass the aggregate validation limit. Array products, strip
sums, vector element storage and dependency storage are checked before allocation.

The partition `array_bytes` limit and `retained_bytes` diagnostic cover retained
partition records, source arrays, hashes and dependencies. They exclude the two
temporary relation maps in `partition::graph::resolve`. Those maps group the
already decoded skin instances by partition and geometry owners by instance.
Their key and value counts are bounded by the admitted skin block and owner
counts, each at most the scene block limit (100,000 by default). They are linear
scratch storage, released before returning; map-node and vector-capacity overhead
is not an exact byte measurement. A zero partition array budget therefore does
not mean zero temporary allocation. There is no separate partition scratch-byte
limit. Exact scene block-count and retained-array boundaries, including an empty
partition catalogue with zero array/work budgets, are covered by the follow-up
tests. These limits do not establish a whole-process memory ceiling.

Native schema 2 is explicitly tagged
`nv-canonical-flags-four-wide-or-empty` and requires
`raw_partition_fields_checked: true`. The CLI compares each partition's source
block ID/type/span/hash and every source count, presence byte and array exactly,
in addition to the existing skin and owner comparison. Native preflight rejects
unsupported widths/noncanonical flags before upstream bool/array loading and
caps partition storage at 128 MiB. No preparation or source-repair helper runs.
Capture and authored comparison tools independently hash the actual executable
before/after execution and require its embedded hash to agree. This supplements
the CLI's report-provenance checks.

## ASSET-02 private verification

Evidence: `local/asset-02-dev-20261003-01`. The private native executable is
`local/nif-skin-oracle-build-02/Release/nif-skin-oracle.exe`; the ASSET-01 frozen
executables and original captured inputs remain available separately.

- `partition-tests-01.log`: 14 partition tests, including all admitted streams,
  both authored face branches, all truncated prefixes, surplus/count rejection,
  unsupported flags/widths, raw finite weights, every shared owner/instance,
  missing-owner/geometry diagnostics, dismember count mismatches and storage/work
  budget rejection.
- `rust-regressions-01.log`: 66 NIF/container/scene/collision/skin/partition tests
  pass. `cli-tests-01.log`: 12 CLI tests pass. `clippy-01.log` and
  `format-check-01.log`: relevant all-target Clippy and formatting checks pass.
- `authored-partitions-01/summary.json`: 72 exact schema-2 comparisons covering
  six authored variants across twelve streams. Twenty-nine altered reports and
  four malformed/unsupported source variants are rejected. Authored strip packets
  retain a declared triangle count of 42 without derived topology repair.
- `schema1-regression-01/summary.json`: all 48 ASSET-01 comparisons remain exact;
  all 24 deliberately altered schema-1 reports are rejected.
- `retail-comparison-01/summary.json`: the 32 frozen samples from nine archives
  compare exactly for all 750 selected skin/partition blocks and 250 owners.
  Their 606 partitions contain 612,152 weight words, 153,038 vertex-map entries
  and 6,473 palette entries. All 554 strip and 52 triangle packets match. Both
  source dependency catalogues are empty for this sample; runtime readiness
  remains false. No gameplay or pose acceptance is claimed.

The native executable's checked before/after/embedded SHA256 is
`2e81efaa91dad9a8c2da0a39fc84e814429c2e953da82f722fc74965433096b2`.
The reused ASSET-01 source manifest SHA256 is
`fee87b753bc5fad8898baa181ca67a97f50752e2f345a203cd5fed0c6923859c`.
All sampled real files use stream 34, width four and canonical flags; other
admitted streams and absent/empty branches currently have authored coverage.
Whole-corpus, first-person and GRA coverage remain open. This worker has not
rehash-verified the entire installation or published a checkpoint receipt.

The only ASSET-02 shared-root wiring is the additive CLI flag/dispatch argument;
the module export is inside the owned skin module. Public source limits/API and
schema mode were coordinated before implementation. Source lock/parity/checkpoint
updates remain with the primary agent.

## ASSET-03 decoded source bindings

`nif_skin::binding::{decode, decode_with_limits}` returns the index and
`Source { skin: partition::Source, bindings: Catalogue }`. Private
`decode_with_scene` helpers live only in the owned skin modules. The original
scene decoder runs once, supplies the existing strict parent/cycle/footer
validation and source objects, and remains unchanged. Its existing world matrices
are not published or used for skin evaluation by this binding catalogue.

Bindings preserve one entry per authored skin instance, including unowned,
null-root and unresolved instances. Bone entries preserve ordinals, duplicates
and original reference order. Node IDs and source name indices stay distinct;
duplicate names never identify or merge bones. Geometry owners retain their
original source block order. NiNode/BSFadeNode projections preserve source spans,
hashes, name/extra-data/controller/property/collision refs, raw flags, local
transform bits and ordered nullable child/effect arrays. Footer roots retain
nullable positions and source order.

The catalogue and CLI/native schema-3 mode are explicitly scoped to
`decoded-source-forest`. `decoded_root_contains` uses bounded iterative forest
intervals over existing decoded edges. `None` means the root or bone is not
decoded. `false` means no path exists in that decoded forest. An unsupported
intermediate can carry an undecoded connection, so scoped false cannot establish
complete-source non-membership. Unsupported scene edges remain visible and no
missing parent, root or bone is invented. Footer reachability is a separate fact.
Root/bone nulls, undecoded types, footer unreachability and outside-decoded-root
facts are diagnostics; authored references are not repaired or rejected for
those facts. Existing source kind/range errors and scene cycles/multiple-parent
errors still fail through their original validators.

Binding limits include the existing partition limits, a separate 64 MiB source
and graph storage budget, and a 16-million-check work budget. Traversal uses no
recursion. Block-indexed graph vectors, interval/stack scratch, owner grouping,
retained node/reference arrays, instance/bone/owner records, edge strings and
diagnostics are charged. `retained_bytes` conservatively includes charged graph
scratch even after it is released. Empty relations still consume work.

`fallout nif-skin INPUT --include-bindings` implies partition inclusion and
requires native schema 3 with `binding_scope: decoded-source-forest`,
`raw_node_fields_checked: true` and `graph_membership_checked: true`. Source fields,
decoded parents/footer reachability, root/bone resolution and scoped membership
are compared exactly. The native reader loads only selected raw factories;
bounded node preflight rejects malformed spans/counts and nonfinite local floats.
Native parent/reachability traversal and separately budgeted ancestor walks are
independent of the Rust interval traversal. No world/skin transforms are composed
by the added native binding path. Existing schema-1 and schema-2 modes remain
available, and both catalogue and report runtime readiness remain false.

Pinned inherited node predicates were rechecked: NiObject contributes no fields;
NiObjectNET uses header string indices, extra-data arrays and controller refs in
this file version; flags are ushort through stream 26 and uint above 26;
properties remain present through stream 34; collision refs are present for
20.2.0.7; node effects remain present for user 11/stream below 130. BSFadeNode adds
no payload. NiBlockRefArray's empty-reference cleanup runs on writing only, so raw
factory reading retains nullable child/effect positions; authored comparisons
exercise this rather than relying on reconstructed references.

## ASSET-03 private verification

Evidence: `local/asset-03-dev-20261003-01`. Native build output is separately
`local/nif-skin-oracle-build-03/Release/nif-skin-oracle.exe`. Previous ASSET-01/02
frozen executables and captured source inputs remain separate and unchanged.

- `binding-tests-04.log`: 12 tests pass, covering twelve admitted streams, exact
  raw bits/name indices/order, nullable arrays, duplicate names/bones, null and
  unowned/unresolved instances, disconnected/footer roots, unsupported
  intermediates, existing invalid-graph rejection, a 10,000-node chain and exact
  storage/work budget boundaries.
- `rust-regressions-01.log`: 78 NIF/container/scene/collision/skin/partition/binding
  tests pass. `cli-tests-01.log`: 12 CLI tests pass. All-target Clippy with warnings
  denied and formatting checks pass in `clippy-01.log`/`format-check-01.log`.
- `authored-bindings-01/summary.json`: 169 exact independent comparisons, comprising
  fourteen graph variants across twelve streams plus a 10,000-node chain.
  Thirty-five deliberately altered reports are rejected. Node/root/bone/owner
  identity, raw arrays/bits, footer positions, graph scope and membership
  provenance are included in those rejection checks.
- `schema1-regression-01/summary.json`: 48 original comparisons and 24 deliberate
  corruption rejections pass. `schema2-regression-01/summary.json`: 72 original
  comparisons, 29 corruption rejections and four source rejections pass.
- `retail-comparison-01/summary.json`: all 32 frozen original files compare exactly
  for 1,742 selected skin/partition/node source blocks. Their 992 decoded nodes,
  250 instances, 2,121 ordered bone references and 250 owners match independent
  source graph facts. All sampled bones/owners are inside their decoded roots;
  binding diagnostics, unsupported scene edges and source dependencies are zero.
  Complete-source ancestry, poses, runtime and gameplay remain unaccepted.

The native executable's before/after/embedded SHA256 is
`024b025b85bd62d5dafa92455614165ac4b361cfb0dafe540660165c7d1b52a4`.
Real-source coverage remains the same bounded stream-34 sample from nine archives;
the other stream/negative graph cases are authored. Whole-corpus, first-person,
GRA, external skeleton binding and evaluated transforms remain open.

The additive `--include-bindings` CLI flag/dispatch argument is the only shared
root wiring. Scene access/API, source binding limits and schema mode were approved
before implementation. No scene, runtime/save, preview, parity or source-lock file
is edited in this lane.

## V3-ASSET-01 source-local palette and CPU deformation

`nif_skin::pose::evaluate(bytes, source, Request, Limits)` reuses the existing
scene, partition and binding decoder once. The request identifies one exact
geometry block and supplies either `PreserveRawNonnegative` or
`RequireUnitSum { absolute_tolerance }`. Both policies retain every contribution
and duplicate influence. The latter checks the raw sum and never divides by it.
Missing weights, negative weights, unweighted vertices and unresolved source
ancestry refuse with a specific error. An undecoded scene edge prevents this
initial evaluator from certifying complete ancestry, even if it is elsewhere in
the file. External skeleton binding remains unsupported.

The receipt uses `engineering-source-local-skin-v1`, original NIF axes/units and
column-vector affine rows. For each authored bone ordinal its skin-space palette
is `SkinTransform * BoneToRoot * SkinToBone`. `BoneToRoot` composes decoded node
locals up to, excluding, the selected skeleton root. The last two source transform
roles are described in the pinned nifly `include/Skin.hpp`; field decoding uses
the fully inspected `src/Skin.cpp` and admitted XML branches. The implementation
is original Rust math with independently authored analytic fixtures, without
copying or linking the reference implementation.

Returned positions are the raw weighted sum of transformed source points.
Normals are weighted linear directions without inverse-transpose correction or
renormalization; that explicit engineering rule does not establish retail normal
behavior. The returned `skin_to_source_world` is
`RootToSourceWorld * inverse(SkinTransform)`. Presentation applies this matrix
once to the evaluated skin arrays, then its existing source-coordinate conversion
and placement. It must not apply the geometry object's local/world matrix again.
Source rotations, reflection and scale remain intact; singular/overflowing skin
mappings refuse rather than receiving an identity fallback.

Stored source locals need not equal the original bind pose. Controllers remain
unapplied, with exact object/controller IDs recorded for every required ancestry
path. This static source-local request does not sample a clip, drive a clock,
infer transitions/events or claim measured retail playback. ASSET06/09 sampling
will enter through the separate explicit-time source/controller request.

Limits nest the existing bounded source decoder and add a 64 MiB element-storage
budget, 16 million work units and an ancestry depth ceiling of 1,024. Element
accounting includes temporary block maps, returned palette/vertex arrays, receipt
and controller records; allocator capacity/overhead is not a process memory cap.
Source scans, ancestry steps, palette entries, influences and final vertex checks
consume work, including empty relations. No recursive traversal is introduced.

The existing three CLI source schemas remain unchanged. A separate single-file
mode selects `fallout nif-skin INPUT --pose-geometry BLOCK
--pose-weight-tolerance TOLERANCE`; both arguments are required together.
Source-oracle/partition/binding flags cannot be combined with this mode. Its
receipt binds the whole input hash, exact geometry/data/instance/root IDs, palette,
raw sums, evaluated arrays and display transform. Refusal emits an error receipt
and exits nonzero. The callable API is the presentation skin consumer boundary;
the diagnostic exposes the same production math for reproducible checks.

The focused handoff checks pass 57 skin/partition/binding/pose tests, including
11 new analytic pose cases, plus 36 CLI tests, affected-package Clippy with
warnings denied and formatting. Analytic cases distinguish root, geometry and
skin spaces; nested rotation/scale, changed source locals, reflection, duplicate
weights, absent normals, unresolved ancestry, singular transforms and exact
budget ceilings have explicit expectations.

`tools/nif-skin-oracle/check_pose.py` checks one preserved stream-34 original
input against the already independently decoded native schema-3 source fields.
Its full 4x4 rational calculation folds the hierarchy from root to bone, while
Rust prepends locals while walking toward the root. All 25 palette entries and
300 coefficients pass the declared binary64 forward-error bound; maximum
absolute error is `1.247655587073829e-14`. The evaluated source has 1,706 vertices.
The admitted raw unit-sum tolerance, exactly `2^-23`, comes from this selected
source's maximum error and does not normalize weights. Zero-tolerance and missing
geometry requests refuse, and an intentionally changed palette coefficient fails
the independent comparison. Frozen binary, input and native report hashes remain
unchanged. Receipts stay in ignored `local/v3-asset-01`; this selected engineering
comparison establishes neither whole-corpus pose support nor retail playback.

For that same immutable input, the new frozen CLI and preserved ASSET09 binary
emit byte-identical source receipts in all three schema modes. Both missing-pair
cases and all three conflicting source flags refuse at argument parsing. Three
failed private compatibility harness attempts remain preserved: they mismatched
oracle schema/count admission or compared schema-1 dependencies to schema-3
retired dependencies. The final check compares matching modes through both
frozen binaries; these failures did not require a production change.

## One source-linked sampled node

`pose::evaluate_sampled(bytes, source, SampledRequest, animation_request,
CombinedLimits)` bridges the existing same-container explicit-time evaluator
into the selected skin. `SampledRequest` requires the expected 32-byte source
SHA, the existing geometry/weight `Request`, and the explicit
`ControllerPolicy::RefuseOtherRequired` policy. `animation_request` is the
existing exact object/controller/finite source-time request. The bridge calls
the existing evaluator internally; a caller-provided `ObjectPose` or matrix
cannot substitute validated source authority.

The returned `EvaluationWithSample` preserves the complete source-linked sample
receipt and the skin output, labeled `engineering-one-linked-sample-skin-v1`.
Only the selected validated node-local transform replaces a stored local. Bone
chains rebuild `SkinTransform * BoneToRoot * SkinToBone` in authored order. A
sampled root or ancestor above it changes `RootWorld * inverse(SkinTransform)`;
the root local does not also enter the relative palette. The sampled node must
occur on a selected bone-to-root path or the root-to-footer path. A sibling
outside those paths refuses, even if its channel can be sampled independently.
Geometry owner transforms remain excluded from deformation/display composition.

All other controllers needed by root, owner or bone ancestry refuse with exact
object/controller IDs. The selected controller's raw clock/flag fields and
interpolator constants/WXYZ words stay in the sample receipt, unapplied as before.
Linear1/constant5 translation/scale and absent-group NiAV retention keep their
existing engineering rules; rotation keys, controller chains and controlled
sample ancestors remain unsupported. Normals remain raw weighted linear
directions, without inverse-transpose or normalization. A zero sampled scale is
allowed as a forward deformation; singular authored SkinTransform still refuses
because the display mapping requires its inverse. Stored-local `evaluate` and
source-report schemas retain their behavior.

`CombinedLimits` admits both decoders' complete declared array allowances and
decoder/index/sampler check allowances before either decode. These are
conservative admission envelopes, not measurements of allocated bytes or elapsed
work: animation uses its combined key/animation allowance plus scene arrays;
skin uses scene, skin, partition and binding allowances. Checked addition rejects
overflow. Default envelopes are 640 MiB and 72 million checks; actual default
allowances total 544 MiB and 67 million checks. Existing decoder input/block caps
separately bound index and scene graph tables. Source input is borrowed once and
capped through all decoder paths before SHA scanning. The bridge separately sums
charged pose/skin helper and output storage (including scratch already released)
under a 72 MiB default, and pose traversal under 18 million units. Each next
evaluation receives only the remaining allowance before output allocation.
Allocator overhead, vector spare capacity, source/index block-count tables and
process memory are outside the element-array count; those existing source caps
remain enforced. Any failure returns no `EvaluationWithSample` or partial palette.

The headless consumer is:

```text
fallout nif-skin INPUT --sampled-pose-request REQUEST.json --output RECEIPT.json
```

The strict schema1 request requires `expected_source_sha256` (32 byte integers),
`geometry`, `absolute_weight_tolerance`, `object`, `controller`, `source_time`,
and `controller_policy: "refuse_other_required"`. Missing/extra fields, another
policy/schema and conflicting source/static-pose options refuse. Input is capped
at 64 MiB, request JSON at 64 KiB; output must be outside both source and request
directories. An evaluation refusal emits `evaluation: null`, exact error and
nonzero exit. This does not select actor state, equipment, a live clock or events,
or assert measured retail playback. Presentation retains GPU and placement
ownership.

Authored two-bone checks exercise noncommuting transforms, changed weighted
vertices/normals, unchanged other-bone contributions, exact endpoints, sampled
root and above-root mapping, identity/link/refusal cases and exact aggregate
ceilings/one-over. `tools/nif-skin-oracle/check_sampled.py` constructs a second
independent sibling-bone source and checks literal vertex/normal expectations at
negative, zero and positive sampled scales through the frozen CLI.

The focused validation passes 39 data tests (seven new sampled-skin cases),
38 CLI tests, affected all-target Clippy with warnings denied, formatting and
the CLI build. The frozen second fixture passes three analytic deformations and
nine intended identity/link/time/schema/policy/protected-directory refusals.
The selected preserved original geometry's stored-pose receipt and source schemas
1/2/3 stay byte-identical to the preceding frozen binary. Its native-confirmed
root has no controller; an explicit sampled-root request refuses that missing
link with no partial pose. No supported retail animated-skin playback is claimed.
Immutable receipts and the frozen executable stay in `local/v3-asset-16`.

## Exact compact raw influences

`influences::prepare(bytes, source, geometry, Limits)` constructs a private
source-bound `Table` through the existing binding/skin/scene decode. Its identity
records whole-source SHA, selected geometry/data/instance/skin-data IDs and bone
count. Read-only `vertex_offsets()` and `entries()` expose CSR ranges: vertex `v`
uses entries `[offsets[v], offsets[v+1])`. Each entry retains exact bone ordinal,
binary32 weight bits and the physical weight ordinal inside that bone's source
array. Serialization is an observation; there is no deserialization or public
authority constructor.

Preparation validates and counts all influences before allocating entries, then
builds checked prefix sums and fills each vertex in original bone/weight order.
Duplicates, more than four influences, nonunit raw sums, +0 and -0 entries remain
intact. No merging, magnitude sort, pruning or normalization occurs. Every vertex
must have a positive finite raw sum. Missing positions/weights, negative/nonfinite
weights, invalid vertex/bone links, unresolved source ancestry, mismatched bone
arrays and count/prefix arithmetic overflow refuse.

`Table::evaluate(bytes, source, pose::Request, pose::Limits)` requires the same
source SHA and geometry before invoking the existing private palette/scene
pipeline. Exact instance/data/bone/vertex identity is rechecked inside that
pipeline. The table supplies its CSR influences directly to the shared existing
weighted-point/direction accumulation and raw-sum policy checks. It does not run
a second reference deformation or accept matrices from the caller. The source
decoder and palette are still evaluated on each call; the table makes no playback
or speed claim. Per-vertex order equals the previous bone-owned accumulation
order, preserving binary64 arithmetic. Normals retain the existing weighted
linear-direction contract, without inverse-transpose or normalization.

Default preparation caps four million entries, 32 MiB charged helper/output
elements and sixteen million traversal units. Before decoding it admits the sum
of the declared existing source array/check allowances, capped at 512 MiB and
64 million units; defaults declare 448 MiB and 48 million units. Independent
source input/block/decoder caps remain active. `Usage` reports actual charged
skin/partition/binding elements separately, the conservative full decoder
allowances, table logical output bytes, charged arrays including temporary
counts/sums, entry count and work. Allocator overhead, spare capacity and
block-count-bounded scene/index tables are outside these counts. Preparation and
evaluation are separately bounded phases; evaluation also admits retained table
output plus its pose arrays together under `pose::Limits.array_bytes`. A failure
returns no completed table/deformation.

The strict headless consumer is:

```text
fallout nif-skin INPUT --influences-request REQUEST.json --output RECEIPT.json
```

Schema1 JSON requires `expected_sha256` as 32 byte integers, exact `geometry` and
explicit `weights`: `{"kind":"preserve_raw_nonnegative"}` or
`{"kind":"require_unit_sum","absolute_tolerance":0}`. The latter only validates
the raw sum and never changes weights. Request/source caps are 64 KiB/64 MiB;
output must be outside both input directories. This option conflicts with source
reports and other pose requests. Reports contain the complete table, source-local
reconstruction and usage; a later reconstruction failure emits `evaluation:
null` and nonzero exit. GPU storage/upload choices, actor selection, animated
playback and retail deformation remain separate.

`tools/nif-skin-oracle/check_influences.py` constructs a second sparse source
with noncommuting bone transforms and seven influences on one vertex, including
duplicates and signed zero. Literal expected positions/normals/sums exercise raw
order and nonunit accumulation through the real consumer. Preserved native skin
weights provide an independent exact entry/offset projection for a selected
installed source; decoded source weights do not prove gameplay skinning.

Focused validation passes 55 pose/clip/skin integration tests (six new CSR
cases), the checked-prefix overflow unit test, 38 CLI tests, affected all-target
Clippy with warnings denied, formatting and the CLI build. The frozen second
source passes its literal sparse reconstruction and nine intended refusals.
For original source `618eb19e...`, all 3,552 entries and offsets match the
preserved native decoder's 25 bone arrays over 1,706 vertices. Source-local
positions, normals, palettes, source frame and sums agree with the existing
evaluator on both sources. Earlier authored/original pose receipts and source
schemas1/2/3 remain byte-identical. Final checks and frozen artifacts stay in
ignored `local/v3-asset-18`; no measured retail or GPU skinning claim is made.

## Explicit two-source rig mapping

`external::evaluate(skin_bytes, rig_bytes, source, &Request, Limits)` evaluates
one source geometry against explicit node IDs from a second source. The caller
supplies both exact whole-source SHA256 values, the skin geometry, rig root,
one mapping for every skin bone ordinal and an explicit finite, invertible
root-space affine matrix `X`. Each mapping includes exact expected raw name bytes
for its selected original skin bone and rig node. Names are checked after ID
selection, never searched or used to choose a rig. Duplicate raw names at
different explicit IDs are valid; duplicate ordinals or rig targets refuse.
Missing names, wrong hashes/types, incomplete mappings, unreachable ancestry,
cycles and unsupported scene edges return no completed evaluation.

`X` maps rig-root coordinates into the skin source's declared root coordinates.
Each engineering palette is `S * X * RigBoneToRoot * SkinToBone`: `S` and
`SkinToBone` come from the original NiSkinData; rig-relative transforms come
from the selected rig's authored ancestor chain. The selected rig root's own
local transform is excluded. Geometry-local transforms are excluded from the
palette. The existing source-world display mapping remains
`SkinRootWorld * inverse(S)` and is applied once. Invertibility of `X` is validated
as part of this explicit relationship, although its inverse is unused in forward
palette construction. Finite shear and reflection are admitted.

The explicit external `X` gate now requires a conservative binary64 interval
for its exact determinant to exclude zero. Every product and addition/subtraction
extends its rounded bounds to outward adjacent representable values, including
subnormal results. Nine bounded interval products and five additions/subtractions
charge fourteen mapping admission units. A zero-containing or nonfinite enclosure
refuses as singular or uncertifiable, including some truly invertible but
numerically uncertain requests. This uses no scale epsilon. The existing finite
inverse check follows certification; the existing stored-source SkinTransform
inverse and all prior palette/report contracts remain unchanged.

Raw influence accumulation reuses the existing private pose helpers in original
bone/weight order. Duplicates, signed zero and nonunit sums remain intact;
`RequireUnitSum` validates without changing weights. Normals use the existing
weighted linear direction contract. This producer does not call a second stored
pose evaluator, accept caller-provided palettes, select actor/equipment state or
apply controller sampling. Required rig controllers and original skin root/owner
controllers are recorded as unapplied observations.

The receipt includes both whole-source hashes, explicit mapping, raw names and
source-qualified node spans, selected rig root span, unapplied rig root local and
source-world matrices, each rig-relative and authored bind matrix, palettes,
deformed source-local positions/normals and the original skin display mapping.
The nested skin palette's `node` IDs refer to the separately identified rig
source. Its usage fields repeat the aggregate producer usage, rather than
describing an independently executed evaluator. The contract is
`engineering-exact-external-rig-skin-v1`; retail behavior is unverified.

Default bounds admit 128 MiB combined source bytes, 4,096 mappings, one MiB of
caller name bytes, ancestry depth 1,024, 64 MiB charged extra logical elements and
sixteen million traversal units. Existing source decoders keep independent caps.
Before decoding, their declared array/check allowances are admitted together
under 640 MiB and 64 million units; defaults declare 576 MiB and 48 million units.
Block-count-bounded scene/index tables remain separately governed by the source
decoders. Private maps, path walks, controller observations, name copies and
deformation arrays are charged. Reported storage is logical accounting including
temporary elements, excluding allocator overhead and spare capacity; it is not
a process-memory ceiling or playback-speed measurement.

The owned headless consumer is:

```text
fallout nif-skin SKIN --external-rig RIG --external-skin-request REQUEST.json --output RECEIPT.json
```

Strict schema1 JSON requires `expected_skin_sha256`, `expected_rig_sha256`
(32 byte integers each), `geometry`, `rig_root`, `explicit_bone_mapping`,
`explicit_root_space_mapping` and explicit `weights`. Each mapping contains
`bone_ordinal`, `rig_node`, `expected_skin_bone_name_bytes` and
`expected_rig_node_name_bytes`. Weight policies match the compact-influence
consumer above. Both flags must be supplied together and conflict with other
source/pose modes. Each input is capped at 64 MiB and request JSON at 64 KiB;
output must be outside all three input directories. Semantic refusal emits
`evaluation: null`, exact diagnostic and nonzero exit.

`tools/nif-skin-oracle/check_external.py` constructs independent named skin and
rig sources with noncommuting transforms, an explicit rig root, duplicate names,
non-UTF8/NUL bytes and an explicit shear/reflection mapping. It checks literal
deformations for distinct explicit targets, complete mapping permutation and
identity/type/name/ancestry/schema/policy/protected-directory refusals.

Final validation passes 63 focused pose/clip/skin integration tests (six new
external-rig cases), 38 serial CLI tests, affected all-target Clippy with warnings
denied, formatting and the CLI build. A fresh frozen executable on the corrected
pose-set base passes two literal second-source deformations, complete mapping
permutation and twenty intended refusals. Earlier source schemas1/2/3, stored
pose and full compact-influence receipt remain byte-identical.

For installed childfemaleupperbody source `618eb19e...` and male skeleton
`c6667dd9...`, a frozen explicit engineering identity `X` produces 25 palettes
over 1,706 vertices. All 300 palette coefficients agree with independently
decoded native source transforms under exact-rational forward-error bounds;
maximum absolute error is `3.581417024842903e-14`. Deliberate coefficient mutation
is rejected. All 28 required rig controllers remain unapplied. This validates
source composition; original deformation, actor/child alignment, placement,
rig parity and retail playback are unmeasured. Raw string capture used an already
frozen pinned native executable without rebuilding it. Initial test expectation
failure, original draft proofs and byte-exact preservation/restoration before
the pose-set correction remain immutable beside fresh validation03, binary02
and consumer02 evidence in ignored `local/v3-asset-20`.

The coordinator's independent exact integer witness found that the original
rounded cofactor predicate admitted singular `X` when row3 equals row1+row2.
Frozen20 reproduces that admission; the corrected gate above refuses it. Fresh
validation passes 72 focused data tests (two additional certification cases),
38 serial CLI tests, Clippy, formatting and a rebuilt frozen consumer. An exact
rational reference proves the witness determinant zero, two nearby +/-1 matrices
invertible, four admitted extreme/reflection cases invertible, and five refused
uncertain requests also invertible. The latter are deliberate conservative
refusals. The public authored consumer now passes its prior literal deformations
and permutation plus twenty-three refusals. Original native palettes retain the
same independent error bound and all earlier source/stored-pose/CSR receipts
remain byte-identical. Original20 proofs, first fixture-path reproduction failure
and test-loop Clippy failure remain preserved; fresh evidence is in
`local/v3-asset-20/root-mapping-correction-02`. This correction affects only new
external root mapping admission, without changing the existing pose inverse.

## Reusable source and shared-skeleton geometry batches

`pose::PreparedSkinSource::prepare(bytes, source, PreparationLimits)` owns the
existing NIF index, skin/binding catalogue, Scene and a bounded validated geometry
owner map. The private-field object retains no borrow of caller bytes and exposes
no public decoded-catalogue constructor, serialization, mutation or global cache.
`source_sha256()` and `usage()` are read-only observations. Exact source spans,
binary32 words and decoded arrays remain owned after the caller changes or drops
its input. Preparation records one existing binding decode and one Scene decode;
subsequent evaluations borrow those same internal catalogues.

`PreparedSkinSource::evaluate_many(source, expected_source_sha256, &[Request],
BatchEvaluationLimits)` requires the preparation's exact whole-source identity
and a nonempty, ordered set of unique explicit geometries. Each geometry keeps
its own geometry/data/instance/skin-data/root IDs, source bone order, authored skin
mapping, weight policy, normals and placement. All numeric accumulation uses the
existing private evaluator. A second geometry never inherits a first geometry's
palette or raw weights. Complete observation permutation changes only output
order. Later failure returns no completed batch. Preparation remains immutable;
a failed evaluation does not publish partial geometry output.

`pose::evaluate_many(bytes, source, expected_source_sha256, &[Request], BatchLimits)`
is the convenience preparation/batch wrapper. Its private preparation verifies
the exact expected hash before decoding. Existing one-shot, sampled-source and
compact-influence paths retain their original validation order and observations;
their shared numeric body now receives a private borrowed decoded view. No public
matrix or catalogue becomes source authority.

Preparation independently bounds the existing source decoder and admits its
declared array/check allowances under defaults of 512 MiB and 64 million units
(448 MiB and 48 million units declared by the default decoder). Additional header,
hash and owner-map elements default to four MiB; source hash byte visits, owner
admission and map initialization to 128 million units. Input and block/index/Scene
caps remain active. Batch evaluation separately defaults to 64 geometries,
128 MiB charged elements and 128 million units, with the existing per-geometry
64 MiB, sixteen million units and depth1,024 caps. Complete batch output headers
are admitted before allocation; each next geometry receives remaining aggregate
allowances as well as its own caps.

`GeometryBatch.preparation` describes the completed preparation phase. Batch
`retained_bytes`/`work_units` describe the current evaluation phase, including
charged temporary elements and complete output headers. The separate source
binding retention figure includes existing skin/partition/binding elements,
excluding independently block-bounded index/Scene storage. Preparation usage is
repeated as provenance on each evaluation, not charged as decoding that ran again.
These logical counters exclude allocator overhead and spare capacity and do not
measure process memory or playback speed.

The owned consumer is:

```text
fallout nif-skin INPUT --shared-skin-request REQUEST.json --output RECEIPT.json
```

Strict schema1 requires `expected_source_sha256` (32 byte integers) and
`geometries`, each with exact `geometry` and an explicit raw `weights` policy.
Input/request limits are 64 MiB/64 KiB; output must be outside both directories.
Other source/pose modes conflict. Semantic refusal returns `evaluation: null`,
exact error and nonzero exit. Controllers remain recorded as unapplied; source
local engineering deformation does not establish animated actor or retail
playback. `tools/nif-skin-oracle/check_shared.py` provides a second independent
two-geometry source with different mappings and noncommuting transforms, literal
positions/normals/palettes/placement, full permutation and request refusals.

Validation passes 70 focused pose/clip/skin tests (seven new shared-source cases),
38 serial CLI tests, affected all-target Clippy with warnings denied, formatting
and the CLI build. Tests drop/mutate the caller's input, reuse one preparation,
exercise independent and aggregate exact/one-under caps, and preserve complete
permuted observations. The frozen second source passes two literal geometries,
both full palettes, full permutation and twelve intended refusals.

Installed source `4c89ebbb...` has geometries1/47 sharing root0. Their 37/15
palettes, 444/180 coefficients and 2,954/392 vertices retain independent source
identity and raw weights through one preparation. All palette coefficients meet
the exact-rational bounds from preserved native source transforms, with maximum
absolute error `7.530293869397177e-15`; deliberate mutation is rejected. Raw sum
error is at most `7.450580596923828e-08`, checked with the explicitly recorded
tolerance `1.9371509552001953e-07` without altering weights. Each original pose,
earlier source schemas1/2/3, sampled-skin receipt and full compact-influence receipt
remain byte-identical to frozen20. Engineering output does not establish original
deformation or playback. Immutable checks and the frozen executable stay in
ignored `local/v3-asset-13`. This single producer also closes the coordinator's
refined reusable-source alias ASSET21.

## Source-qualified partition rows and draw indices

`partition::streams::prepare(bytes, source, Request, Limits)` produces a sealed
`Streams` from the existing partition decoder and its existing Scene. The request
requires the exact whole-source SHA256, geometry ID, linked partition block and
physical partition ordinal. Read-only identity, presence, palette, vertex,
influence, topology and usage getters expose observations. Private fields and no
deserialization prevent an outside catalogue from becoming source authority.

The identity contains independent geometry/data/instance/partition block spans
and payload hashes, physical partition ordinal and source vertex domain. Every
local palette entry retains its authored global bone ordinal and decoded source
node. Every physical vertex slot retains local/source vertex indices, slot order,
local/global bone indices, node ID and binary32 weight word. Duplicate palette
entries, vertex mappings and bone slots remain duplicated. Signed zeros, finite
negative weights and non-unit sums remain raw. This producer performs no numeric
weight policy or deformation; the existing skin decoder still refuses nonfinite
weights. Missing source bone links and wrong node types refuse.

Triangles retain local and mapped source index triples. Strips retain authored
lengths and both complete ordered index streams, including repeated and degenerate
indices. No triangulation or winding rule is inferred. The independent authored
triangle-count word remains visible even when it differs from a simple strip
length calculation. All four presence flags must explicitly admit their arrays;
absent arrays are never synthesized. Empty present arrays retain the unused width.
The admitted nonempty NV branch uses four weight slots per vertex. Exact shared
owner constraints from the existing decoder apply; mismatched selected dismember
body-part counts refuse before this producer returns a stream.

Checked row products and topology counts default to four million influences and
four million indices. All output and temporary elements are charged before
allocation under 32 MiB, including headers, raw/mapped strip copies, palette/vertex/
influence rows, span hash strings and the decoded-node membership map. Hash byte
visits, owner lookup, map construction, row projection and draw-index projection
default to 128 million work units. Strip-length/count work is charged before the
index-count scan, including strips with no indices. Decoder array/check allowances
are separately admitted under 448 MiB and 32 million units; default declared
allowances are 384 MiB and 32 million units. Existing 64 MiB source input and
block/index/Scene limits remain active. Source retention reports the existing
skin and partition element counts; index/Scene storage remains independently
bounded. These logical element/work counters include charged temporaries and
exclude allocator overhead and process-memory measurements. Failure returns no
usable `Streams` or partial output.

The owned consumer is:

```text
fallout nif-skin INPUT --partition-streams-request REQUEST.json --output RECEIPT.json
```

Strict schema1 requires `expected_source_sha256` (32 byte integers), `geometry`,
`partition_block` and `partition_ordinal`. Request/input limits are 64 KiB/64 MiB;
output must be outside both input directories. Other source/pose modes conflict.
Semantic refusal records `evaluation: null` and an error with nonzero exit.
`tools/nif-skin-oracle/check_streams.py` supplies independent container spans and
literal authored rows for both topology branches, repeated palette and vertex
mapping, raw weight words and explicit request/domain refusals. This stream is an
engineering source projection. Renderer adoption, partition deformation, shader
weight/normal rules, original draw behavior and retail playback remain separate.

Validation passes 93 focused partition/skin/animation tests, including six new
stream cases, 38 serial CLI tests, affected all-target Clippy with warnings denied,
formatting and the CLI build. Exact and one-under input, decoder admission, row,
draw-index, element-storage and work ceilings are checked independently. The
first stream-test run's wrong expected error remains preserved: the existing skin
decoder correctly refused a wrong bone type before the new producer. A later
budget-order tightening charges strip work before scanning; both full validation
runs remain immutable, with the final executable frozen after the second run.

The second independent source passes three complete literal partition outputs,
36 raw influence rows, sixteen intended refusals and byte-identical earlier
schemas1/2/3. Selected installed source `618eb19e...`, geometry1, partition block8,
ordinal0 matches preserved pinned-native evidence exactly for 159 vertex rows,
five palette entries, 636 raw influence rows and all 420 authored strip indices.
Deliberately changed weight/index words fail the independent comparison. Earlier
installed-source schemas1/2/3, stored pose and compact-influence receipts remain
byte-identical to frozen corrected20. The native projection charges 25,460 logical
element bytes and 360,078 work units; those numbers do not measure process memory
or rendering speed. Frozen executable, commands, hashes and full immutable
receipts stay in ignored `local/v3-asset-22`. Retail behavior remains unverified.

## Deformed geometry projected through an authored partition

`pose::partition::evaluate(bytes, source, Request, Limits)` accepts exact whole-
source SHA256, the existing explicit geometry/weight-policy request, linked
partition block and physical ordinal. It decodes once through the existing
binding/partition/Scene pipeline and calls the existing private geometry pose
evaluator once. No outside palette, matrix, catalogue or public receipt is
accepted as authority. This adapter uses the existing CPU `NiSkinData` deformation,
then selects positions, raw weighted normals and raw weight sums through the
authored partition vertex map. It does not substitute the partition's weight
words or invent a hardware shader rule.

Each selected vertex carries partition-local and source vertex IDs. Repeated
source IDs remain repeated; a source-to-partition CSR map records every occurrence
in authored local order, including source vertices with no selected occurrence.
Triangles/strips retain original local indices and strip lengths. Source body-part
flags and identifiers are observed without selecting visibility or dismemberment.
The exact palette and source-world placement stay in the existing skin-root frame;
apply placement once. Missing normals remain `null` on every selected row. Source
controllers remain recorded as unapplied. Owner/link/ordinal/palette mismatches,
missing vertex-map/faces and mismatched dismember counts refuse atomically.

Limits separately bound the existing full pose (64 MiB/sixteen million units),
additional subset elements/work (32 MiB/128 million units), and their aggregate
(96 MiB/144 million units). Whole-source hash visits and source/reverse-map work
are charged in the adapter phase. The full pose's entire charged intermediate
arrays/work remain counted even when the selected output has one vertex. Reverse
maps are also charged across the full source vertex domain. Selected vertex and
draw-index ceilings default to65,535/four million. Decoder allowances default to
448 MiB/48 million declared, admitted under512 MiB/64 million. Existing input,
block, index and Scene limits remain active. Output arrays and temporary cursors
are admitted before allocation; later failure publishes no partial subset.
Counters describe logical elements/work, including released intermediates, rather
than process memory or rendering speed.

The owned consumer is `fallout nif-skin INPUT --partition-pose-request REQUEST.json
--output RECEIPT.json`. Strict schema1 requires the exact `expected_source_sha256`,
`geometry`, `partition_block`, `partition_ordinal` and existing explicit `weights`
policy. Source/request limits are64 MiB/64 KiB; output stays outside both input
directories, and other pose/source modes conflict. Semantic refusal records a null
evaluation and nonzero exit. `tools/nif-skin-oracle/check_partition_pose.py` supplies
a second independent authored source and literal noncommuting subset coordinates,
palettes, normals, raw non-unit sums, placement, CSR mappings and topology. This
producer establishes an engineering subset; original partition deformation,
weight/normal/shader rules, rendering, body-part behavior and playback remain
unverified.

Validation passes 99 focused partition/skin/animation tests, including six new
subset cases, 38 serial CLI tests, affected all-target Clippy with warnings denied,
formatting and the CLI build. Independent exact/one-under caps cover source input,
decoder admission, full deformation, subset elements/work and aggregate output.
A one-vertex subset of a4,096-vertex source still charges the entire deformation
and source-domain reverse map. The initial test-build missing trait import is
preserved; the corrected full validation passes without changing deformation.

The second independent source passes three literal subsets (eight vertex rows),
noncommuting full palettes/placement, raw non-unit sums, typed topology, reverse
mapping and fifteen intended refusals. Earlier source schemas and stored pose
remain byte-identical. Installed `618eb19e...` geometry1/block8/ordinal0 projects
159 vertices from the previously frozen1,706-vertex engineering CPU pose at the
exact vertex indices in preserved native evidence. Native body-part fields, five
palette entries and all420 strip indices are retained; source-index, position and
strip mutations are rejected. This comparison establishes source-index projection
equivalence, not original deformed coordinates or a shader rule. Earlier source
schemas1/2/3, stored pose, compact influences and partition-stream receipts remain
byte-identical to frozen22. Full deformation charges99,834 element bytes/7,104 work
units, and the additional subset charges40,648/198,751, both present in aggregate
usage. Commands, hashes, frozen executable and immutable receipts stay in ignored
`local/v3-asset-23`. Gameplay/playback acceptance remains unchanged.
