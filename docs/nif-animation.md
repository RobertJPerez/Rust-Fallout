# NV animation source framing

## V3-ASSET-02 exact external clip/skeleton pose

`nif_animation::clip::evaluate` connects an explicitly selected controlled entry
from one clip container to an explicitly selected node in a second skeleton
container. Its contract is `engineering-exact-external-clip-pose-v1`. The caller
supplies both expected SHA256 values, skeleton object ID/raw name bytes, clip
sequence ID/controlled ordinal and finite binary64 source time. Coordinator
outbox102 approved this source boundary: actors supplies record-specific choices;
assets validates the binding/math; presentation applies selected transforms once.
Equipment selection, scheduling and retail-time policy remain outside this API.

Hashes are checked before source decoding/result allocation. Existing keyframe
and scene decoders fresh-index their respective immutable sources. The selected
skeleton object must be a decoded node with the exact caller name and that name
must be unique among decoded skeleton nodes. The exact controlled packet must
name the same bytes and declare `NiTransformController`. Initial external support
requires no controller/property/controller-ID/interpolator-ID binding. Exact
`NiTransformInterpolator` and authored `NiTransformData` links are mandatory;
there is no guessed alternate object, name search or static-interpolator fallback.

Rotation keys and spline orientation remain precise refusals. Translation/scale
use the existing linear1/constant5 source sampler at caller time. Empty groups
retain the corresponding authored NiAV component under the preceding ASSET03
engineering rule. Raw interpolator constants/WXYZ words remain unapplied.
Source NiAV rotation, sampled scale and translation produce the local affine;
static ancestor locals produce source-world coordinates. Controlled ancestors,
unknown scene edges and unreachable destinations refuse. Private component and
ancestry helpers are shared with the preceding pose command; no duplicate parser
or separate hierarchy implementation is added.

Receipts bind both source hashes and correctly scoped object/sequence/interpolator/
data spans, exact controlled packet/ordinal/name, original locals/flags, raw
sequence clocks/weight/notes and interpolator fields, key diagnostics, static
ancestry, matrices and logical budget usage. Object controller IDs are retained
unapplied. Caller time is never remapped through frequency, phase, cycle, sequence
weight or event annotations. `retail_behavior_verified` remains false.

Admission inherits ASSET03's 64MiB source caps, 16,384 blocks, 32MiB scene/source
logical arrays, 64MiB combined clip-key retention, one million respective key/
sample/pose work allowances, 4MiB extra result/maps/name storage and 1,024 ancestry
depth. Combined source input is additionally128MiB. Name uniqueness compares
each stored skeleton string once into a bounded match table, then visits nodes;
repeated name IDs cannot multiply long comparisons. Source string bytes are
bounded by input, table entries by storage/work. Logical charges include temporary
maps/tables and exclude allocator overhead/vector spare capacity; they are not a
process-memory measurement.

`fallout nif-clip-pose SKELETON CLIP --request REQUEST_JSON --output REPORT`
reaches the production evaluator. Strict schema1 JSON requires SHA256 byte arrays,
`object`, `node_name_bytes`, `sequence`, `controlled_ordinal` and `source_time`.
It admits64KiB request JSON and64MiB per input, and protects every source/request
tree before report publication. The coordinator's exact binary64 JSON feature
commit `2a865269a09e6b6b2c599e9a6b85a54e96173aeb` is a consumed dependency;
the owned draft was preserved across the clean boundary and restored byte-for-byte.

All 21 focused data tests pass (nine new clip cases, twelve prior pose cases),
with 38 CLI tests, affected-package all-target warnings-denied Clippy, formatting
and build. `tools/nif-animation-oracle/check_clip.py` provides a second authored
source pair. Frozen verification passes five independent literal poses, thirteen
intended binding/time/source refusals and one prescribed binary64 transport word.
All five preceding positive/refused pose receipts remain byte-identical, including
their budgets, and selected original animation source schemas1/6 stay byte-identical.

The actual preserved first-person pair is `meshes/characters/_1stperson/1gtaim.kf`
(archive entry10446, SHA256
`9fed7a15222c449f568e74daa0cafe98fc8acfda190d83262caa90883f5a4cc7`)
and `_1stperson/skeleton.nif` (entry10514, SHA256
`3fe5a3ef9718c8bff773b328c93bf6e522e85b16afd0de1b9af33cfba550b121`).
Independent preserved native fields confirm sequence0/controlled1 -> interpolator2
-> data3, raw `Bip01 Rotate` name equality and the unique skeleton node12. The
source has XYZ rotation axes with quadratic2 keys; the bound request returns the
exact sequence/ordinal/interpolator/data rotation refusal with no partial pose.
This resolves source binding while retaining a real orientation coverage gap;
it does not establish first-person retail playback. No corpus reinventory, retail
input mutation or gameplay acceptance is implied. Evidence remains local in
`local/v3-asset-02`, with original sources and frozen binaries unchanged.
The first private consumer run passed its source/math checks but reused a loop
variable when writing the summary's node-name field. That incorrect summary
remains preserved. A fresh corrected run records the exact byte array and passes
all comparisons without Rust, input or frozen executable changes.

## V3-ASSET-10 exact rigid attachment from stored locals

`nif_animation::attachment::evaluate` takes immutable skeleton and attachment
containers, both expected SHA256 values, an exact skeleton node ID and raw name
bytes, an exact attachment footer-root ID, caller affine placement and the
explicit `StoredNiAvLocals` source policy. Its contract is
`engineering-source-local-rigid-attachment-v1`. Expected hashes are checked
before source decoding or additional result allocation. Existing `nif_scene`
decodes both containers; assets supplies no second importer, socket lookup,
actor/equipment selection or persistent pose authority.

The selected skeleton object must be a decoded reachable node with the exact
raw name. Explicit block identity resolves duplicate names; no search, case
folding or string conversion changes the selection. The attachment object must
be a decoded node and an exact parentless footer root. Unknown scene edges and
unreachable objects refuse. The result retains the selected-node-to-footer
path, root block spans/hashes, raw local transform words, flags and unapplied
controller IDs. Stored source locals can already be animated; this request
does not establish a bind pose, evaluate controllers or invent missing sockets.

Let `N` be the selected skeleton node's stored source-world affine, `P` the
explicit caller matrix from attachment source-parent axes to node axes, and
`R` the attachment root's authored local. The result separately exposes:

- `attachment_source_to_skeleton_source = N * P`, for attachment meshes whose
  existing source-world transforms already include `R`.
- `root_to_skeleton_source = N * P * R`, for coordinates local to the root.

Both use original source axes/units and column-vector affine rows. The consumer
applies the first mapping to existing source-world meshes once. Applying the
second to those meshes repeats the root transform. Finite shear, reflection,
nonuniform and zero forward placements are admitted without inverse,
orthogonalization or normalization; nonfinite inputs and accumulated overflow
refuse. Runtime/world placement and presentation coordinate conversion remain
their respective consumers' explicit operations.

Default source admission is 64MiB per container, 128MiB combined input, 16,384
blocks and 32MiB logical scene arrays per container. Attachment additionally
bounds result/maps/hash/name storage to 4MiB, source-list/map visits, ancestry
and affine coefficients to one million work units, and skeleton path depth to
1,024. Hash work is bounded by admitted input size. Logical charges exclude
allocator overhead and vector spare capacity; they are not a process-memory
measurement. Failed evaluations return no partial mapping and never mutate
input. Whole-source hashes remain part of every successful receipt.

`fallout nif-rigid-attachment SKELETON ATTACHMENT --request REQUEST_JSON
--output REPORT` reaches the production API. Schema1 JSON requires both
`expected_*_sha256` arrays of32 byte integers, `node`, `node_name_bytes`,
`attachment_root`, the 3x4 `attachment_parent_to_node` matrix and
`source_policy: "stored_ni_av_locals"`. Unknown/missing fields and unsupported
schemas refuse. JSON is bounded to64KiB and inputs to64MiB each. Reports bind
all three input hashes and cannot be created inside any source/request tree.
The independent `tools/nif-animation-oracle/check_attachment.py` uses a second
authored pair with literal noncommuting matrix expectations. All results retain
`retail_behavior_verified: false`; saved equipment, animated sockets and retail
first/third-person attachment behavior remain separate work.

All 21 focused attachment/pose tests pass, including nine new attachment cases,
with 36 CLI tests, affected-package all-target warnings-denied Clippy, formatting
and build. The frozen independent consumer passes three different authored
placements and twelve intended identity/name/root/schema/publication refusals.
Inputs and frozen executable stay unchanged. One initial fixture compile error
(`.5` instead of `0.5`) is preserved alongside the corrected passing check in
`local/v3-asset-10`. This is a bounded engineering producer; authored agreement
does not establish original equipment selection or visible retail animation.

## V3-ASSET-06 explicit-time local visibility

`nif_animation::visibility::evaluate` connects the existing raw Boolean/key
decoders to one exact same-container scene object. The caller supplies object
and controller block IDs plus a finite binary64 source time. The contract is
`engineering-linked-local-visibility-v1`: the object must point directly to a
`NiVisController`, its target must be that object, its next-controller link must
be absent, and its interpolator must be `NiBoolInterpolator`. The object must
be reachable from the footer and unresolved scene edges refuse. Timeline
crossing, controller chains and manager state remain unavailable.

The pinned NV `NiVisController` has the existing `NiTimeController` and single
interpolator layout, with no legacy `NiVisData` field. That shared reader is
reused without adding visibility to the default four-class source catalogue.
Reserved flag bits, undefined cycle type3 and ManagerControlled refuse. The
remaining recognized flag and clock fields are retained unapplied. This API
evaluates a source channel at caller time; it does not advance a controller,
compute effective parent visibility or interpret the source object flags.

Linked `NiBoolData` must have nonempty constant tag5 keys with finite strictly
increasing times and raw values0/1. Exact binary32 key times promote to
binary64; the last key at or before caller time supplies the local Boolean.
Both endpoints are included and extrapolation refuses. A single key requires
its exact time. Duplicate/decreasing times and raw values2..255 refuse for the
entire selected group, even if the invalid key would not be sampled. Keys are
never sorted or repaired. Without a data link, only an authored interpolator
pose0/1 is admitted at any finite time. Linked empty data does not fall back to
the pose value. The source default/sentinel2 remains unavailable.

The result carries whole-container SHA256, object/controller/interpolator/data
block IDs and span hashes, caller time bits, selected key/index bits or explicit
authored-pose selection, raw Boolean, local visibility, raw object flags,
interpolator value, unapplied controller fields and logical work/storage usage.
`retail_behavior_verified` remains false. Consumers must bind the whole source
identity and apply their separate parent/actor visibility state explicitly.

Default bounds are 16,384 source blocks, 32MiB source/scene logical arrays,
64MiB combined retained animation/key source, 16MiB Boolean key arrays and one
million Boolean key visits. Visibility adds a 1MiB logical result budget and
one million work units, charging source-list visits, checked controller links,
selected key validation and each binary-search iteration. All decoding borrows
the same immutable bytes through existing decoders. These are logical retained
budgets, excluding allocator overhead; they are not a process memory cap.

`fallout nif-source-pose INPUT --object ID --controller ID --source-time TIME
--local-visibility --output REPORT` emits a separate schema1 contract receipt.
Omitting `--local-visibility` keeps the preceding explicit object-pose command.
Failures retain whole-source identity and a contextual error, with no partial
visibility result. `tools/nif-animation-oracle/check_visibility.py` authors a
second small source independently and checks literal held-state boundaries,
constant poses and intended refusals using a frozen production executable.
Engineering sampling and measured original playback remain separate gates.

All 26 focused data cases pass (eleven new visibility tests, eight raw Boolean
tests and seven Boolean-key tests), along with 36 CLI tests, affected-package
all-target Clippy with warnings denied, formatting and build. The independent
frozen CLI checks pass nine authored cases and fifteen intended refusals.
The preceding pose command produces byte-identical negative/positive/endpoint
and outside-range receipts, and original player-skeleton source schemas1/6
remain byte-identical to the preceding frozen binary.

The preserved actor manifest's player skeleton (archive entry19065, source
SHA256 `c6667dd94fd10392f851f748438b7c69c0d2cb407448becae6431d5ed1994c4c`)
contains a real chain: node134 points to transform controller136, which points
to visibility controller137. Independent preserved native node/controller
fields confirm these exact links. Selecting controller137 directly refuses
the object/controller mismatch. Chain resolution remains a concrete coverage
gap; this refusal does not establish visible retail animation. Input, native
receipts and frozen binaries stay unchanged. Private evidence is retained in
`local/v3-asset-06`, including one wrapper rejection before Cargo caused by an
incorrect worker status spelling, corrected before the passing run.

## V3-ASSET-03 explicit-time linked object pose

`nif_animation::pose::evaluate(bytes, source, Request { object, controller,
source_time }, Limits)` connects the existing ASSET06 component sampler to one
exact same-container source object. Its `engineering-linked-source-pose-v1`
receipt is an engineering transform request. It does not establish retail
controller playback, animation clocks, events, transitions or external clip
binding.

The object must reference exactly the requested decoded `NiTransformController`;
the controller must target exactly that object, have no next controller and link
to a decoded `NiTransformInterpolator` with `NiTransformData`. Source blocks and
ancestry come from the existing decoders, each indexing the same immutable input.
No public predecoded index, bone/node name or fallback object can replace these
links. Unknown scene edges, disconnected objects and any controller on a required
ancestor refuse. Rotation keys also refuse: their mapping remains unapplied.

Translation and scale reuse the bounded linear/tag-1 or constant/tag-5 sampler at
the caller's finite binary64 source time. Time remains in the source key domain;
frequency, phase, start/stop and flags are recorded without application. Exact
endpoints retain source values; outside-key-range requests refuse. An absent
group retains that component from the object's NiAV local transform. Interpolator
constants, including exact raw WXYZ quaternion words, remain recorded unapplied.
This explicit engineering rule supplies no default retail pose.

The evaluated local matrix retains the authored NiAV rotation matrix, selected
scale and translation, in original source axes/units and affine column-vector
rows. Static ancestor locals compose to `source_world`, nearest parent first;
the selected object's stored world matrix is never reapplied. Signed and zero
forward scale remain valid; nonfinite accumulated matrices refuse. The skin and
object evaluators share private owned affine math. Presentation uses the returned
matrix for the exact selected object and applies its coordinate/placement mapping
once. This initial object request does not animate a whole skin palette.

The receipt binds the whole input SHA256 and exact object/controller/interpolator/
data block IDs, offsets, lengths and hashes. It includes raw local fields, flags,
unapplied interpolator/controller fields, per-channel source-key diagnostics,
ordered static ancestors, matrices and logical budget usage. Fresh source identity
is established for every call; failed evaluation returns no partial pose.

Default admission is 64 MiB input, 16,384 blocks, 32 MiB scene arrays, 64 MiB
combined animation/key retention, one million key/pose/sampling work units per
respective allowance, 1,024 ancestry depth and 4 MiB extra pose/maps/string storage.
The scene's block-bounded object/traversal tables and allocator overhead are
separate from its payload-array charge. These logical limits are not a measured
process-memory ceiling. Hashing is bounded by admitted input size.

`fallout nif-source-pose INPUT --object BLOCK --controller BLOCK --source-time
TIME --output NEW_REPORT` exposes the same production evaluator in a separate
receipt. It requires all three explicit arguments, emits contextual failures with
nonzero exit, and leaves the seven existing animation source schemas intact.
The relevant pinned `Animation.hpp` and `Animation.cpp` were read completely;
source layout facts are reused through existing original Rust decoders, without
copying upstream implementation. Stored clock fields do not define an admitted
retail-time mapping.

The focused engineering tests pass all 12 new linked-pose cases plus 13 existing
keyframe and eight sampler cases. Independent analytic expectations cover two
different source transform spaces, exact links/hashes, signed parent scale and
rotation, retained locals for absent groups, step endpoints, binary64 time detail,
controller/ancestry/rotation refusals and exact/one-under storage/work/depth limits.
`tools/nif-animation-oracle/check_pose.py` supplies a second authored packet with
literal analytic matrices for a different parent rotation and signed/zero sample
scales. Frozen consumer verification and original-source coverage stay separate
from measured retail playback.

The frozen CLI passes all five second-fixture analytic poses, including negative
and zero scale, and seven intended source/time/link refusals. The original
Protectron skeleton from the preserved actor manifest (root `FalloutNV.esm:17A03`,
archive entry 6636, source SHA256
`559049dbac7b44fdc98fb71a38c03fdc7c538755712b77526578f63740e3bceb`)
refuses three concrete unsupported requests: object 4's actual controller 7 is
outside the admitted transform class; selecting controller 8 instead mismatches
the exact object link; object 16's controller 18 has no interpolator. Existing
independent native node/controller projections confirm those exact links. The
new frozen CLI and preserved ASSET09 binary emit byte-identical schema-1 and
schema-6 animation source reports for this original input. All binary/input/native
hashes remain unchanged.

All 36 CLI tests, affected-package warnings-denied Clippy, build and final format
checks pass. One failed consumer harness is retained: it attempted to write a
report beside its generated input, which the existing CLI source protection
correctly refused. The corrected checker writes inputs to their own directory.
Ignored receipts remain under `local/v3-asset-03`. Original controller chains,
external skeleton/clip links and missing interpolators remain real pose coverage
gaps; source refusal is not visible animation or gameplay acceptance.

ASSET-04 decodes four exact source classes: `NiTransformController`,
`NiControllerSequence`, `NiTransformInterpolator` and `NiTextKeyExtraData`.
It preserves fields, authored order and finite binary32 bits. Animation clocks,
interpolation, events, name binding, evaluated poses and gameplay remain unverified.

## Admitted source layouts

The unchanged NV container reader admits file `20.2.0.7`, little endian, user
`11`, and Bethesda stream `14,21,24,25,26,27,28,30,31,32,33,34`. No other tuple
falls back to these layouts. Source facts were read from pinned
[nifxml](https://github.com/niftools/nifxml/blob/970a6238218a106daaeb89a61bcda0eeaf9d08c4/nif.xml)
`970a6238218a106daaeb89a61bcda0eeaf9d08c4`, `nif.xml` SHA256
`d6b76a83ea5fbadd21da5f06348b236a1c43da6bb76d108a2e8b749c22618dd6`,
and the selected complete inherited definitions in
[nifly Animation.hpp/Animation.cpp](https://github.com/ousnius/nifly/blob/cca0a770094bb962fb28ea1fec5ea903e68fda8e/src/Animation.cpp),
`ExtraData.hpp/ExtraData.cpp`, `Keys.hpp`, `BasicTypes.hpp/BasicTypes.cpp` and
`Object3d.hpp`, revision `cca0a770094bb962fb28ea1fec5ea903e68fda8e`.
No upstream parser algorithm is copied into Rust. The dedicated GPL native oracle
stays separate from the replacement runtime.

| Exact class | Fields present in the admitted tuple |
| --- | --- |
| `NiTransformController` | Next-controller ref, raw ushort flags, frequency/phase/start/stop floats, target pointer, interpolator ref: 30 bytes |
| `NiControllerSequence` | Name string index, uint controlled-block count, uint array-grow value, ordered 29-byte controlled packets, 32-byte timing/link tail, stream-dependent note links |
| `NiTransformInterpolator` | Translation, quaternion W/X/Y/Z, scale, transform-data ref: 36 bytes |
| `NiTextKeyExtraData` | Extra-data name string index, uint key count, ordered time/string-index pairs of eight bytes |

`NiObject` and the interpolator base classes contribute no stored fields here.
The controller's `NiTimeController` prefix is 26 bytes. Its inherited
`managerControlled` field exists only in file `10.1.0.104..108`, and the old
keyframe-controller data ref only below `10.1.0.104`; both are absent here.
The interpolator ref is present from `10.1.0.104`.

Each controlled packet has interpolator/controller refs, a priority byte and
five header string indices: node name, property type, controller type, controller
ID and interpolator ID. Old inline target names, blend fields, string palettes
and palette offsets are absent at `20.2.0.7`. The sequence tail is weight,
text-key ref, raw uint cycle tag, frequency, start, stop, manager pointer and
accumulation-root name index. Old phase/play-backwards/palette and newer
accumulation-flags branches are absent.

Note links are absent for streams 14/21, a single nullable ref for streams
24..28, and a ushort-counted ordered ref array for streams 30..34. Absent,
present-null and present-empty layouts remain distinct. Nullable array positions
and duplicate packets/links are preserved. `BSAnimNotes`/`BSAnimNote` payloads
are unparsed dependencies. The pinned XML and nifly disagree about type-1
`BSAnimNote` gain/state fields; no body layout is inferred to hide that finding.

The transform's older `TRS Valid` booleans are absent at this file version.
Signed zero, subnormal values and finite extreme sentinel words remain exact.
Quaternions are not normalized, and sentinels do not establish channel validity.
Text keys keep duplicate/unsorted times and raw string indices. Their bytes do
not establish event syntax or delivery. Unknown cycle words are preserved with
a diagnostic rather than assigned guessed clock behavior.

## API, links and bounds

`nif_animation::{decode, decode_with_limits}` accepts source bytes and returns
the unchanged `NifIndex` plus `Animation`. Selected blocks retain source IDs,
type names, offsets, lengths and SHA256 values. The source catalogue always has
`runtime_ready: false`. No scene decode, transform composition or runtime/save
adapter is added.

Nonnull links have their exact index ranges checked. A metadata-only table of
555 pinned XML class/inheritance facts distinguishes known wrong-family targets
from unknown classes. Known wrong families fail. Known unparsed payloads and
unknown classes remain distinct explicit dependencies. Inheritance facts imply
neither admitted target payload versions nor evaluator support. Standalone
clips legitimately contain null controller/manager links; nulls stay raw source
fields without invented missing-object policy. Controller self-links remain
source facts, with no evaluated-chain policy. Every controlled packet retains an
external-binding dependency; name indices do not identify or merge scene nodes.

Defaults are 64 MiB input, 100,000 container blocks, 128 MiB logical array/string
storage and 16 million source-reference work units. An allocation-free owned
metadata preflight bounds counts before calling unchanged `nif::inspect`.
It charges predictable retained index vectors/strings and logical block-count
map records, and admits concurrent temporary type-index/size vectors plus a
temporary type-name clone. Existing type/string/group/root caps remain in force:
16,384 types, one million strings/groups/footer entries and 16 MiB string bytes.

Temporary index charges are released after construction. `retained_bytes`
conservatively charges the returned index and source catalogue, including hashes,
arrays, dependency type strings and diagnostics, but excludes released scratch.
Map nodes, allocator rounding/capacity and caller input storage are not an exact
byte measurement or whole-process memory ceiling. These remain count/input
bounded. The opaque-index test demonstrates why the construction budget can
exceed the retained diagnostic even when no selected animation blocks exist.

Each source array is checked against its span/element/storage bounds before
allocation. Dependencies are first counted and family-validated without allocation,
then filled into reserved vectors. This takes a bounded constant number of
source visits. Reference work includes null block/string indices, selected
blocks, controlled packets and empty controlled/text/note products. Malformed
counts, truncation, surplus bytes, nonfinite floats, bad indices/kinds and budget
exhaustion fail with source context. Finite timing/weight values are not repaired
or assigned gameplay meaning.

## Raw strings and independent comparison

`NifIndex.strings` already contains arbitrary byte vectors. The animation decoder
reuses those exact bytes, including non-UTF8/CP1252, embedded/trailing NUL and
CR/LF/quote/backslash characters. It performs no decoding or normalization.

The pinned `NiString::Read` removes exactly one trailing NUL. The native oracle
therefore independently scans the original bounded header before factory
allocation, retains raw string vectors, checks `GetStringById` against the
corresponding one-NUL-trimmed interpretation and emits the original byte arrays.
It reports a normalization count. Reconstructed normalized strings are never
treated as proof of raw bytes. Other source counts are independently preflighted
and checked against factory vector counts.

`fallout nif-animation INPUT [--oracle-report REPORT]` uses source schema 1,
branch `nv-four-source-classes`, float encoding `ieee754-binary32-bits` and string
encoding `raw-byte-arrays`. Native provenance requires the pinned revision,
`prepare_data_called: false`, raw-string/count checks and false runtime readiness.
It compares all selected source block fields/spans/hashes, tuples, raw global
string bytes and normalization counts exactly. Corrupt/missing/duplicate evidence
and source errors exit unsuccessfully. Dependencies remain visible even when
source fields agree.

The CLI caps batches at 10,000 inputs and conservatively charges each returned
index/catalogue against a 256 MiB aggregate budget, including raw string vector
and byte storage. Native headers and selected factory storage each have a
separate 128 MiB admission budget; factory structure sizes can produce narrower
admission than Rust's source structs. Native per-file JSON buffering is capped
at 64 MiB before append; the CLI admits oracle JSON at most 64 MiB. These are
separate logical limits, not identical allocator accounting. No full-corpus
batch capacity is claimed. Native reading uses raw factories only, with no
`NifFile::Load`, `PrepareData`, string-ref filling, cleanup or evaluation.

## Private development evidence

Evidence resides in the asset worktree at `local/asset-04-dev-20261003-01`, with
native build `local/nif-animation-oracle-build-04`. These are worker development
checks, not an integration checkpoint or accepted gameplay scenario.

- Fourteen authored decoder tests pass, covering all twelve stream revisions,
  raw bits/strings, ordered/duplicate/empty data, all typed roles/ranges,
  malformed source fields and exact storage/work/input/block boundaries.
- `rust-regressions-01.log`: 93 NIF/container/scene/collision/skin/partition/
  binding/animation tests pass; the original-data sampler is explicitly ignored
  without supplied inputs. Twelve CLI tests, all-target data/CLI Clippy and
  formatting checks pass in their separate logs.
- `family-facts-01.log` checks all 555 metadata facts against the pinned XML;
  the private Rust family test also checks sorted uniqueness and classification.
- `authored-comparison-01/summary.json`: 96 exact comparisons (eight variants
  across twelve streams), 59 altered reports and seven malformed sources rejected.
  NUL-normalization supplements, CP1252/control bytes, sentinel words and all
  absent/null/empty note branches are exercised.
- `retail-comparison-01/summary.json`: 70 original files, 2,678 selected blocks,
  3,045 controlled packets, 213 text keys, 4,216 strings and 59,377 raw string
  bytes compare exactly. The 6,264 dependencies consist of 3,219 unparsed payload
  links and 3,045 external bindings; diagnostics are zero and runtime readiness
  remains false.

The retail sample is deterministic and bounded: 40 KF and 30 NIF files selected
from nine of eleven examined archives, with 52,176,180 decoded bytes scanned
under a 64 MiB cap.
It includes fourteen humanoid, twelve creature and fourteen first-person KF
files, plus two character, ten creature, four first-person and fourteen other
NIF files. Sixty-eight files use stream 34 and two use stream 33. Other streams,
nonnull note links and NUL/CP1252 strings have authored coverage in this slice.
The read-only source preflight saw 19,329 finite float words, including 11,480
extreme sentinel words and six signed-zero words; independent field comparison
preserves these exactly. Sampling is not complete actor/attachment coverage.

Source manifest SHA256:
`4d53c00ecdc7217bf9acd8764543dea231e4f16d9714db1428e8b8cf321fcdb1`.
Native executable before/after/embedded SHA256:
`56b8f0c1180f52f9365800957bafec6ebd6cd96ff5f993bbcecdaafad0d21ae2`.
Earlier skin binaries/evidence remain frozen. The prior expanded corpus census
retains six unsupported `20.0.0.4` containers; this slice adds no legacy fallback.

Run from the assigned asset worktree:

```powershell
$env:CARGO_TARGET_DIR = 'G:\Rust-Fallout-worktrees\assets\target'
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked -p fallout-data --test nif_animation
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-nif-animation-oracle.ps1
py -3 tools/nif-animation-oracle/check_family_facts.py
py -3 tools/nif-animation-oracle/check_comparison.py --output-dir local/new-animation-run
```

For a fresh original-data sample, set `ASSET_ANIMATION_ARCHIVES` to a JSON array
of at most sixteen explicit archive paths and `ASSET_ANIMATION_EVIDENCE` to a new
directory whose existing parent is under this worktree's `local`. Run the ignored
`nif_animation_samples` test with `--ignored --nocapture`. It selects up to two
sources from the first 48 sorted candidates per archive/category, at most 128
sources and 64 MiB decoded scan bytes. Selection uses the existing production
archive/container readers and retains failures; it does not filter by successful
new animation decoding. Missing original input is never a passing evidence gate.

```powershell
py -3 tools/run-nif-animation-oracle.py --input local/new-sample/inputs --output local/new-sample/oracle.json
target\debug\fallout.exe nif-animation local/new-sample/inputs --oracle-report local/new-sample/oracle.json --output local/new-sample/rust.json
```

Minimal shared wiring is the `pub mod nif_animation` export and CLI
module/command/dispatch. The primary agent owns their integrated versions and
source-lock, parity and checkpoint updates. Key data, compressed/B-spline fields
and numerical sampling need their own following slices. Pose adapters and
required actor-manifest coverage need separate coordinated contracts.

## Exact transform-key source extension (ASSET-05A)

`nif_animation::keyframe::{decode, decode_with_limits}` adds exact
`NiTransformData` source decoding to the existing four-class catalogue. Its
`Source` contains the unchanged `Animation` plus a key `Catalogue`. The original
entrypoints and inspector/native schema 1 retain their source behavior; key
payloads remain opaque unless the optional extension is selected. Both
catalogues always report `runtime_ready: false`.

The admitted container tuples above remain unchanged. Complete inherited
`NiKeyframeData`, `NiTransformData`, `QuatKey`, `Key`, `KeyGroup`, `TBC`,
`Quaternion` and `KeyType` metadata were read from the same pinned XML, with
complete `Animation.hpp`, `Animation.cpp` and `Keys.hpp` inspected from the
same nifly revision. No legacy layout or class-name alias is inferred.

The leading uint rotation count remains exact. Count zero has no stored tag or
keys. Quaternion tags 1/2/3/5 preserve ordered time and W/X/Y/Z bits; only tag 3
stores a tension/bias/continuity triple. Quaternion tag 2 has no stored tangents.
Rotation tag 4 requires the evidenced leading count of one and preserves three
independent X/Y/Z scalar groups, including empty axes. The old rotation-order
float is absent at this file version. Each scalar/vector group preserves its
declared count and optional tag: zero count omits the source tag, tag 2 adds
forward/backward values and tag 3 adds TBC values. Tags 1/5 add neither.

Unknown tags and unevidenced XYZ leading counts are explicit unsupported
branches. Truncation, surplus bytes, count/span violations and nonfinite words
fail contextually. Signed zero, subnormals, finite extremes, zero/nonunit
quaternions, duplicate/unsorted times and original array order remain exact;
nothing is sorted, normalized or repaired. Decoding does not establish angle
conventions, interpolation, clock scaling, event delivery, root motion or poses.

The native extension uses the raw `NiTransformData` factory without loading a
scene or calling `PrepareData`. Before factory allocation, an independent
bounded raw scan checks every count/tag/span and finite word. It supplements the
leading XYZ count that nifly discards and distinguishes absent zero-count tags
from nifly's default `NO_INTERP`. The earlier raw-string supplement is unchanged.
After all admitted key blocks parse successfully, only dependencies targeting
decoded `NiTransformData` retire. Other known unparsed/unknown links and external
bindings remain visible. A known wrong target family still fails the original API.

Key defaults are 128 MiB additional logical array storage, 256 MiB combined
animation/index/key retention admission and 16 million work units. Original
animation limits and index scratch admission are reused once. The combined cap
also bounds animation construction, so scratch can require more admission than
the reported retained total. Key arrays receive at most the checked remaining
combined allowance. Work charges one unit per selected key block, rotation/group
product and stored float word, including empty products. Selected-block and
array-word work is admitted before their respective vectors allocate.
`Catalogue.work_units` records successful admitted work. These are logical
storage/work limits, with count-bounded allocator overhead, rather than a process
heap measurement.

`fallout nif-animation INPUT --include-keyframes [--oracle-report REPORT]`
selects source schema 2 and branch `nv-transform-data-source`; the independent
oracle accepts the same optional flag. The CLI requires
`raw_keyframe_counts_checked: true`, exact schema/branch provenance and the
existing false runtime claim. It compares block identities/spans/hashes and all
ordered fields exactly, using at most one small temporary key JSON value rather
than duplicating a complete catalogue. The 256 MiB report admission and one
16-million-unit key work allowance cover the whole batch. A failed combined
decode conservatively debits its remaining key allowance, preventing later
failures from repeatedly spending the same work. Schema 1 emits neither the
new key catalogue nor keyframe branch field.

Development verification is frozen under
`local/asset-05a-teamv2-20261003-01`, with separate native build
`local/nif-animation-oracle-build-05a-teamv2`. Earlier ASSET-04 inputs, binaries
and evidence remain unchanged.

- Thirteen independent authored decoder tests pass; the affected NIF regression
  totals 106 passing tests and two explicitly ignored original-data samplers.
- Fifteen CLI tests pass, including exact/one-under multi-file work admission,
  attempted-work exhaustion and schema-1 opacity/field preservation. Affected
  data/CLI all-target Clippy with warnings denied, formatting and diff checks pass.
- `comparison-01/summary.json`: 108 authored source files across twelve streams
  compare exactly; 23 changed key reports and eight malformed sources fail for
  their intended reasons. Schema 1 continues to admit those opaque key payloads.
- The same comparison checks all 1,499 key blocks in 70 preserved original files,
  including 133,174 quaternion keys, 71,345 translation keys and 1,072 scale keys.
  Original-file schema-1 native rows agree with the frozen ASSET-04 oracle except
  the actual executable digest; the new Rust schema-1 inspector accepts that
  frozen oracle unchanged.
- `schema1-authored-01/summary.json`: the existing 96 source fixtures still compare
  exactly, with 59 changed reports and seven malformed sources rejected.

The source sample covers 63 files with key blocks and 554 XYZ blocks; it remains
the bounded ASSET-04 sample described above. Original interpolation, evaluated
poses, compressed/B-spline payloads and retail animation behavior remain separate
unverified gates. This handoff is development source evidence, not an integration
checkpoint or gameplay acceptance.

For new team builds, use the current central operating contract's focused-build
wrapper, an explicit private Cargo target and `--locked --jobs 2`. Frozen binaries
can run the independent comparison without rebuilding them:

```powershell
py -3 tools/nif-animation-oracle/check_keyframes.py --output-dir local/new-key-source-comparison --binary local/asset-05a-teamv2-20261003-01/binaries/fallout-asset05a.exe --oracle local/asset-05a-teamv2-20261003-01/binaries/nif-animation-oracle-asset05a.exe
```

## Compact-transform/B-spline source catalogue (ASSET-05B)

The optional `nif_animation::spline` entrypoint reuses the keyframe decoder and
one immutable container index. Its exact source scope for the admitted NV tuples
is `NiBSplineCompTransformInterpolator`, `NiBSplineData` and `NiBSplineBasisData`.
The first payload is 84 bytes under the pinned inherited layout: start/stop,
data/basis links, translation, WXYZ quaternion, scale, three raw u32 handles,
then six float offset/half-range fields. The data block retains ordered f32 bits
and signed i16 compact values with both source counts. The basis count remains
a raw u32 even for zero or extreme values.

Finite sentinels, inverted time ranges, zero/nonunit quaternions, all handle bits
and -32768 compact values are preserved. No valid channel range, sentinel
interpretation, normalization, decompression or usable cubic basis is inferred.
Null links remain absent; unknown target classes produce ordered dependencies.
Known wrong data/basis classes and out-of-range references fail contextually.
Only fully decoded compact-transform interpolator dependencies retire after all
selected payloads and links validate. Compact float/point3 and uncompressed
transform interpolators remain unadmitted payloads. Schema1/2 keep B-spline
payloads opaque.

Spline limits add 128 MiB logical storage and 16 million work units, with one
unit per selected block and stored primitive, including both array counts.
Block work precedes catalogue allocation; complete array count/span/storage/work
admission precedes each control-point vector allocation. The embedded keyframe
combined cap covers index, animation, keys and splines together, including the
existing index scratch admission. Logical charges include retained vector
capacity and owned type/hash strings; allocator overhead is count-bounded rather
than measured. A returned `work_units` receipt describes successful source work.

`fallout nif-animation INPUT --include-splines [--oracle-report REPORT]` selects
schema3, includes keyframes and emits branch `nv-compact-transform-source`.
The independent oracle accepts the same flag. Its raw scan checks counts,
primitive words, links, spans, storage and work before pinned factory allocation.
It uses the pinned XML class-name vocabulary to distinguish unknown targets from
known wrong types. The inspector requires `raw_spline_counts_checked: true` and
exact branch/schema/provenance, and compares each array element without cloning
a complete second catalogue. Combined report retention and separate key/spline
work budgets cover the entire batch; failed decoding exhausts the remaining
work allowances conservatively. Every runtime-readiness flag remains false.

Private development evidence belongs under
`local/asset-05b-teamv2-20261003-01`, with a separate native build directory
`local/nif-animation-oracle-build-05b-teamv2`. No earlier draft, input, frozen
binary or verification directory is overwritten. Source matching remains
distinct from evaluated poses and verified retail animation behavior.

The frozen development comparison in `comparison-01/summary.json` verifies 96
authored files across twelve admitted streams against both readers and the
independent authored expectations. Twenty-two altered reports and fourteen
malformed inputs fail for the intended reasons, including known wrong data/basis
types. Malformed spline payloads remain opaque in both earlier schemas.
The same 70 preserved original files contain 678 compact-transform interpolators,
31 control-point blocks and 32 basis blocks, with 154,885 compact values and no
float control values. Float arrays are verified in the authored scope. All 741
admitted spline block identities/spans/hashes and source fields compare exactly;
the 1,499 original key blocks still match. Original spline work is 169,958 units
and logical retained storage is 475,754 bytes. Remaining dependencies total
4,087, diagnostics zero and runtime readiness false.

Nine new decoder tests and the existing 27 animation/key tests pass. All 17 CLI
tests pass, including two new multi-file work/comparison tests. Data/CLI
all-target Clippy with warnings denied, formatting, pinned class metadata and
diff checks pass. Schema1/2 authored projections are unchanged, the original
schema2 native report matches its frozen ASSET-05A oracle except actual executable
digest, and the Rust schema1/2 original reports are byte-exact with identical
arguments and their frozen oracles. The first decoder run retained one failed
assertion caused
by a misquoted existing tuple-error message; the corrected run passes without
changing source behavior. None of this establishes B-spline evaluation, poses,
rendering or retail animation playback.

## Explicit-time engineering component sampling (ASSET-06)

`nif_animation::sampling` validates a borrowed, immutable scalar or vector source
group before performing engineering linear (tag1) or constant (tag5) math. The
declared count must equal the retained count, every time/value must be finite,
and source times must be strictly increasing. Duplicate/unsorted keys and
unsupported interpolation fields remain visible in the source catalogue but
cannot be evaluated. No sorting, filtering, normalization or key repair occurs.
An absent group returns absence, rather than a substituted value.

Requests use an explicit finite binary64 source time within the source key
range. Singleton groups admit only their exact key time. Exact endpoints and
held constant values retain the original binary32 value bits, including signed
zero. Interior linear results use binary64 weighted interpolation and report
their binary64 bits, both source key indices and alpha bits. This engineering
contract does not assert the retail engine's arithmetic or interpolation policy.

The existing inspector is the immediate consumer:

```powershell
fallout nif-animation INPUT --sample-time 0.25 --sample-block 7 --sample-channel translation
```

All three selection flags are required together; the request accepts one file
and either `translation` or `scale`. It implies keyframe decoding and adds a
separate engineering diagnostic with the exact source block/span/hash and work
receipt. Default source schemas1/2/3 omit those fields. A sampling refusal leaves
the decoded source fields available and reports the failure. Raw native source
comparison and numerical comparison remain separate operations.

Validation and request work each have a 16-million-unit allowance. Validation
admits the complete time/value walk before visiting words; catalogue lookup
shares that allowance. Every request admits search/arithmetic work before doing
it. The view borrows source arrays; the inspector reserves the diagnostic's
64-byte source digest within the existing report cap before source decoding.
Quaternion, quadratic/TBC, extrapolation, B-spline evaluation, pose assembly,
clocks, cycle/event/root-motion policy and rendering remain unadmitted.

Private evidence is frozen in `local/asset-06-teamv2-20261003-01`. Eight sampling
tests, 36 animation/key/spline regression tests and all 19 CLI tests pass, as do
affected all-target Clippy with warnings denied, formatting and diff checks.
The frozen source oracle is the unchanged ASSET-05B executable. A separate
Python reference decodes binary32 words to exact rationals, scans brackets
linearly and evaluates `a + (b - a) * alpha`; it does not repeat Rust's binary
search or weighted expression. Source identities/fields and exact/held bits
match exactly before numerical tolerance is considered.

The comparison checks 213 requests and 497 values: 138 requests/272 values from
nine authored positive files, plus 75 requests/225 values from 16 unmodified
original groups across 14 files. The bounded selection found 246 eligible groups
in the preserved 70-file sample. The justified roundoff bound is
`2^-49 * (abs(left) + abs(right) + abs(exact_result)) + 2^-1074`.
Largest conditioned error is `8.540177112501205e-17`; the original subset's
largest absolute error is `2.3684757858670005e-15`. Authored finite binary32
extremes deliberately yield large absolute rounding errors, so this conditioned
bound is not a universal geometric accuracy claim. Fourteen malformed/unsupported
source or altered numerical/identity reports and five direct CLI boundary probes
fail for their intended reasons. All 70 original default reports remain byte-exact
in schemas1/2/3. These are source and engineering math checks; evaluated poses
and verified retail animation remain separate gates.

## Compact float/point3 source catalogue (ASSET-07B)

The additive `nif_animation::spline::components` decoder reuses the existing
spline/key/animation decoder and its single immutable index. It admits exactly
`NiBSplineCompFloatInterpolator` (32 source bytes) and
`NiBSplineCompPoint3Interpolator` (40 source bytes) under the same pinned NV
tuples. Both retain start/stop bits, data/basis links, base value bits, raw u32
handle and offset/half-range bits. The scalar value is one word; point3 preserves
three words in source order. The pinned XML places the scalar base/handle in the
abstract parent, while nifly reads them in the compact subclass; serialized field
order agrees. This does not admit either abstract/uncompressed source class.

Separate fixed-size component types leave the earlier spline block size and its
logical storage receipts unchanged. Component defaults are 128 MiB additional
logical storage and 16 million work units. The existing combined cap covers the
index, animation, keys, splines and components together. Selected block storage
and digest bytes are admitted before vector allocation; stored primitive work
is admitted before construction (nine units per scalar block, eleven per point3).
Unknown target dependencies remain ordered, with their owned strings/storage
admitted before cloning. Null links remain absent; known wrong types and
out-of-range links fail. Exact interpolator dependencies retire only after every
selected source payload/link succeeds, releasing strings but retaining charged
vector capacity. Handles, sentinel values, inverted times and zero/negative
ranges do not establish valid channels and are never repaired.

`fallout nif-animation INPUT --include-spline-components` selects source schema4,
includes the earlier key/spline catalogues and adds `spline_components` plus branch
`nv-compact-components-source`. The independent native oracle uses the same flag,
checks spans/finite primitive words/links/storage/work before pinned factories,
and emits required provenance `raw_component_fields_checked: true`. The inspector
compares each bounded fixed-size source product exactly. Component work shares
one allowance across the batch; failed combined decoding exhausts the remaining
key/spline/component allowances conservatively. All readiness flags remain false.

Private evidence is frozen in `local/asset-07b-teamv2-20261003-01`, with native
build `local/nif-animation-oracle-build-07b-teamv2`. The raw layout audit checks
20 scalar and ten point3 blocks in eleven unchanged original files, all stream34.
Both readers and independent authored expectations agree for 96 files across
twelve admitted streams. Nineteen altered reports and fourteen malformed sources
fail for the intended reasons; earlier schemas admit those opaque component
payloads. The 70-file original comparison checks all 30 blocks exactly, charging
290 component work units and 5,280 logical bytes. Remaining dependencies total
4,057, diagnostics zero and runtime readiness false. Eight new component tests,
44 prior animation/key/spline/sampling tests and all 21 CLI tests pass. Fresh Rust
and native builds, all-target Clippy with warnings denied, formatting and diff
checks pass. Default schemas1/2/3 original Rust reports are byte-exact; their native
source reports match the frozen oracles except actual executable digest. The
initial private audit's hard-coded header-length mistake and first boundary
test's incorrect storage-error expectation are retained with the passing corrected
runs; neither correction changed original bytes or decoding behavior. Numerical
component/B-spline evaluation, poses and retail playback remain separate gates.

## Archive source coverage audit (ASSET-07)

`tools/nif-animation-oracle/coverage_matrix.py` joins the preserved archive sample
receipts to already compared schema4 animation and schema3 skin reports. It checks
all exact input bytes, hashes, archive member identities, tuples and container
counts. Each inspector receipt must bind the supplied native report hash and
contain a successful comparison. The matrix summarizes those independent checks;
it is not another independent decoder or an actor-root resolver. Source catalogues
remain distinct from evaluated poses and retail behavior.

The input allowance is 128 MiB for the pretty animation report, 64 MiB per other
report and 128 file rows. These limits bound admitted input bytes/rows, rather
than Python process memory. The matrix retains one animation tree without copying
its key or control arrays. Duplicate identities, mismatched source fields, missing
comparison receipts, impossible catalogue counts and readiness claims refuse.
Output creation is exclusive. Optional team authorization and STOP checks apply.

Private evidence is in `local/asset-07-teamv2-20261003-01`. The deterministic
archive-path/category sample contains 70 original NIF/KF members: 14 character,
12 creature and 14 first-person KFs; two character, ten creature, four first-person
and 14 other NIFs. All 70 existing animation source comparisons match. All 30 NIF
skin/partition/binding reports match the unchanged independent skin oracle.
Those NIFs contain 582 binding nodes but zero skin blocks or skin owners: this
sample does not verify first-person or creature skin weights. Actor-root membership
awaits a tested exact archive/member/hash manifest. Relative NIFZ/KFFZ references
remain unresolved; path categories alone cannot establish actor closure.

The source matrix records unparsed animation link occurrences separately from
blocks outside its animation catalogue. It finds 243 Boolean, 173 float and 63
point3 interpolator link occurrences; these counts are not unique block counts.
The creature/first-person categories account for 113 Boolean link occurrences.
An independent bounded header/layout audit finds 252 `NiBoolInterpolator` and
three `NiBoolTimelineInterpolator` blocks across 33 original members, all stream34.
Pinned inheritance and primitive definitions agree on exactly five bytes: raw
uint8 value followed by a u32 `NiBoolData` link. Observed values are 0, 1 and 2;
215 links are null and 40 address `NiBoolData`. This selects a concrete next source
slice. It does not admit Boolean evaluation, timeline events or truth conversion.

Ten altered identity, tuple, comparison, catalogue/readiness and oracle-binding
probes refuse for their intended reasons. The first matrix run refused the
82-MB pretty animation report at its initial 64-MiB admission limit; its snapshot
and failure are retained. The corrected 128-MiB animation allowance and final
matrix pass without changing any original source or inspector output. No Rust
or native implementation changed in this audit.

## Raw Boolean interpolator source (ASSET-07C)

`nif_animation::boolean` adds a separate source catalogue for exactly
`NiBoolInterpolator` and `NiBoolTimelineInterpolator`. Pinned inheritance and
primitive IO agree on five bytes: one raw uint8 value and one u32 `NiBoolData`
link. Values 0 through 255 are retained without truth conversion, including the
source pose sentinel 2. The timeline subclass has no additional stored fields;
its event/update policy remains unverified.

The decoder reuses the existing component/spline/key/animation decoder's single
immutable index. It admits all block/digest storage and five work units per
selected block before allocation or primitive construction. Those units cover
the block, two primitives and two link visits. Defaults are 128 MiB additional
logical storage and 16 million work units, under the existing combined cap.
Dependency vector and owned target names are admitted before cloning. Known
`NiBoolData` targets remain ordered `undecoded_payload` dependencies; unknown
classes remain `unknown_class`. Null links stay absent; out-of-range links and
known wrong classes refuse. Exact incoming interpolator dependencies retire only
after every selected payload/link succeeds. Released strings reduce retention;
the original dependency vector capacity stays charged.

`fallout nif-animation INPUT --include-bool-interpolators` selects additive source
schema5, including the earlier key/spline/component catalogues and a separate
`bool_interpolators` catalogue with branch `nv-bool-interpolator-source`. The
native oracle uses the same flag and requires `raw_bool_fields_checked: true`.
It checks exact spans, links, known classes, work and storage before pinned
uint8 factories, then projects their source values. Fixed-size source products
compare exactly. Batch work shares one allowance; failed combined decoding
conservatively exhausts remaining allowances. Earlier flags omit this catalogue
and keep Boolean payloads opaque.

Private validation is held in `local/asset-07c-teamv2-20261003-01`, with a separate
native build in `local/nif-animation-oracle-build-07c-teamv2`. Eight new decoder
tests include all 256 raw byte values for both classes across all twelve admitted
streams, ordered unsupported targets and exact work/storage boundaries. The
existing 52 animation/key/spline/sampling tests and all 23 CLI tests pass. This
source catalogue does not admit Boolean key evaluation, truth semantics,
timeline events, evaluated poses, clocks, rendering or retail playback.

Both readers and independent authored expectations agree for 96 files across
twelve streams. Thirteen altered reports and ten malformed sources refuse for
their intended reasons; schema4 continues to admit those opaque Boolean payloads.
All 70 originals compare exactly, including 252 ordinary and three timeline
interpolators. Their additional work is 1,275 units and logical retention is
36,680 bytes. Forty `NiBoolData` dependencies remain explicit; overall remaining
dependencies total 3,854, diagnostics zero and runtime readiness false. A third
raw header/layout audit independently matches every Boolean block identity,
span/hash and primitive field. Earlier animation/key/spline/component projections
still match the frozen native oracle, and original Rust default schemas1/2/3/4
are byte-exact. Fresh Rust/native builds, all-target Clippy with warnings denied,
formatting and diff checks pass. The first comparison helper put a malformed
input beside reports and hit the output-directory guard. Its failed snapshot and
run remain preserved; separating input and report directories fixes the evidence
layout without changing decoder code, binary or source bytes.

## Constant Boolean key source (ASSET-07D)

`nif_animation::boolean::keyframes` adds a separate `NiBoolData` catalogue.
The physical zero-count layout contains only its u32 count. Positive counts
require the independently observed constant tag 5, followed by ordered records
of finite binary32 time and raw uint8 value. Other tags refuse explicitly in
this branch and stay opaque through schema5. Raw values 0 through 255, signed
zero, duplicate times and descending times remain unchanged. No sorting,
truth conversion, event timing or Boolean evaluation occurs.

The decoder reuses the immutable index and earlier source catalogues. Selected
block/digest storage, primitive work, the two-million-key count bound and exact
five-byte record span are admitted before key allocation. Work charges one unit
per selected block, one per stored count/tag, and two per key. Defaults add
128 MiB logical storage and 16 million work units under the existing combined
cap. Only fully decoded groups retire exact `NiBoolData` payload dependencies;
unknown classes remain ordered and dependency vector capacity stays charged.

`fallout nif-animation INPUT --include-bool-keys` selects additive source schema6,
includes the earlier catalogues, and emits `bool_keys` with branch
`nv-bool-constant-key-source`. The native oracle uses the same flag and requires
`raw_bool_key_counts_checked: true`. Raw count/tag/span/work/storage preflight
precedes its pinned uint8 key factory. Comparison checks identity, span, digest,
declared count, tag and every ordered key exactly, without duplicating the whole
key catalogue. One work allowance covers the inspector batch; failed combined
decoding conservatively exhausts remaining source allowances.

Private evidence is frozen in `local/asset-07d-teamv2-20261003-01`, with native
build `local/nif-animation-oracle-build-07d-teamv2`. Seven new decoder tests and
the existing 60 animation tests pass, along with all 25 CLI tests. Both readers
and independent authored expectations agree on 96 files across twelve streams.
Sixteen altered reports and seventeen malformed groups refuse for their intended
reasons; schema5 continues to admit these opaque data payloads. All 70 originals
compare exactly, including 40 groups and 1,787 keys. Additional work totals
3,694 units and logical retention is 21,016 bytes. Overall dependencies total
3,814, diagnostics zero and readiness false. A third raw header/layout audit
matches every group identity, span/hash and source key. Fresh Rust/native builds,
all-target Clippy with warnings denied, formatting and diff checks pass.
Original Rust default schemas1/2/3/4/5 remain byte-exact, and their native source
reports match frozen oracles except the actual executable digest. Source keys
remain distinct from sampled poses, timeline events and verified retail playback.

## Actor-root asset source matrix (ASSET-07E)

`tools/nif-animation-oracle/actor_asset_matrix.py` joins tested explicit actor
roots to exact archive candidate tuples and independently compared member bytes.
It consumes an actor projection, member source manifest, preserved inputs,
member backend receipts and existing schema6 animation/schema3 skin reports with
their native oracles. It verifies the tested actor handoff/commit and source
report hashes, then reads bounded original root/comparison ranges directly.
Every ordered path and unresolved status remains intact. These are structural
candidates; the matrix does not select gender, body/head parts, equipment or clips.

Admission bounds are 512 MiB for the streamed actor report, 32 MiB per selected
subtree/projection, 64 roots, 512 path rows/candidates and 128 NIF members. Each
decoded/stored member is at most 64 MiB, with 256 MiB total decoded input. Archive
identity hashing streams at most 8 GiB. Animation and skin report byte limits are
128 MiB and 64 MiB respectively. These are source/input admission bounds, rather
than a Python heap bound. Exact comparison keeps primitive types distinct and
preserves source order; Boolean/integer coercion cannot validate a source receipt.
The matrix checks current archive identity, stored spans/hashes, backend decoded
hash/length/offset, container tuple/counts and independent inspector receipts.

Private evidence is in `local/asset-07e-teamv2-20261003-01`. Tested ACT-07 commit
`886e96674f1dbb98d8730aaf15ce1ea7dac0579f` supplies roots `FalloutNV.esm:7` and
`FalloutNV.esm:17A03`. Their 184 paths retain 130 single archive candidates,
50 unresolved relative bases and four empty paths. Streaming projection retains
533,663 root-array bytes from the 318,741,255-byte frozen source report. The
57 distinct NIF members total 5,446,996 decoded bytes. Independent physical
BSA104/zlib extraction matches the frozen Rust backend at decoded hash/length
and stored offset; the archive hash stays unchanged. All 57 native/Rust skin
and animation source comparisons match. The player root reaches 56 distinct
NIFs; the creature root reaches one skeleton, so creature body skin coverage
remains open. Eighteen skin owners/instances/partitions, 36 skin blocks and 292
binding nodes compare exactly, with zero binding diagnostics or unsupported
scene edges. Structural root membership, evaluated poses and retail behavior
remain separate; no relative base or part choice is inferred.

Eighteen altered projection/member/inspector probes refuse for the intended
identity, order, primitive-type, span/hash, budget, readiness and comparison
binding reasons. The final matrix matches the preceding valid matrix exactly.
Two initial helper failures remain private: a mistaken lookup-field shape in
the projection helper and a receipt byte-count field mismatch in the matrix.
Corrected runs preserve all original source bytes and frozen binaries. This
handoff changes only the owned matrix tool and documentation; existing Rust,
native and default source-report interfaces stay unchanged.

## Explicit engineering cubic components (ASSET-09)

`nif_animation::spline::sampling` borrows a decoded compact component source.
Its named contract is `engineering-open-uniform-cubic-components-v1`. It admits
an exact same-container interpolator/data/basis window with 4 to 2,000,000 basis
controls, checked array cardinality/span, and a finite strictly increasing source
interval. Source array caps still apply, so wider channels can reach the window
limit before reaching the basis-count limit. XML handle 65535 is absent; other
uint values remain offsets and must fit the actual compact array. Unknown links,
float-only controls, short windows and incompatible channels refuse explicitly.
Static source values never supply a fallback.

The engineering algebra is `offset + (signed_i16 / 32767.0) * half_range`, in
binary64 after exact binary32 promotion. It preserves -32768 without clamping
and permits finite negative or zero half ranges. Degree3 open uniform clamped
knots are `[0,0,0,0,1,...,N-4,N-3,N-3,N-3,N-3]`. Caller time maps to
`((time-start)/(stop-start))*(N-3)`, with direct exact-endpoint branches. Local
de Boor uses four rows of four scalars and generated knots. It allocates no
expanded control array, frame bake or knot vector. Time must be finite and
inside the source interval; no extrapolation, cycle, clock or time repair applies.

`prepare` returns a borrowed validated window that supports repeated explicit
requests. Default validation and sampling budgets are 16 million units each.
Validation charges 16 fixed metadata units, each visited source block, and each
selected compact scalar before hashing the window. Every sample precharges
`17 + 10*width` units before component arithmetic or allocating up to four
results. Receipts retain source block IDs, spans/hashes, handle, basis count,
window byte offset/hash, source parameter bits, caller time bits, local control
indices, result bits and exact work usage. Readiness and retail verification
remain false.

The existing `nif-animation` request accepts `--sample-channel spline-translation`,
`spline-scale`, `spline-float`, `spline-point3` or `spline-rotation-components`,
paired with explicit `--sample-block` and `--sample-time`. This opts into schema4
source decoding or preserves a higher source schema selected by its flags.
The optional `engineering_spline_sample` and corresponding contract are separate
from the earlier linear diagnostic. Source/comparison findings remain available
when sampling refuses. Rotation results are four raw WXYZ scalar components;
quaternion normalization, orientation interpolation and evaluated poses are open.
World owns hierarchy/rendering and Runtime owns persistent clocks/state.

`tools/nif-animation-oracle/check_spline_sampling.py` compares Rust requests with
an independent exact-rational Cox basis using exact binary32 parameters and
binary64 caller time. Four-control results also equal independent Bernstein
cubic Bezier expressions. The conditioned component bound is
`2^-42 * max(1,N-3) * (abs(offset) + (32768/32767)*abs(half_range) + abs(exact_result)) + 2^-1074`.
The global source amplitude covers cancellation near a rounded internal knot:
the cubic derivative is bounded by six times that amplitude, and normalized
parameter error grows with N. This is an engineering arithmetic bound. It does
not measure geometric error or verify retail knot, time, rotation or fallback
policy. Authored source/refusal checks and unchanged original-source math checks
remain separate from pose and retail acceptance.

Private ASSET-09 evidence is frozen in `local/asset-09-teamv2-20261003-02`.
Seven new sampler tests and 67 existing animation tests pass, along with all
27 CLI tests, all-target Clippy with warnings denied and formatting. Actual
Rust results match 1,908 independent rational requests containing 5,120 scalar
values. These cover 63 additional authored sources, prepared source vectors
across twelve streams, and 45 original windows spanning observed channel/count
combinations. Tests include finite extremes, negative/zero ranges, subnormal
intervals, -32768, exact endpoints, adjacent knot times, cancellation and a
two-million-control source. All 473 sampling refusals, thirteen malformed-source
refusals and eight altered receipt types in each comparison group behave as
intended. Final original schemas1-6 remain byte-exact across all 70 files.
The first frozen binary/proofs and initial Clippy loop-style failure remain
preserved; a fixed-scratch iterator change passes fresh tests, Clippy and the
full numerical comparisons on a separately frozen final binary. No native
decoder, shared main dispatch, World, Runtime or persistent format changes.

## Source-linked text-key interval observations

`markers::query(bytes, source, Request, Limits)` follows only the selected exact
`NiControllerSequence.text_keys` link through the existing animation decoder.
The request requires the expected 32-byte source SHA, exact sequence block and
finite binary64 `source_start`/`source_end` with start <= end. The read-only query
defines a closed interval in direct source time. Sequence clocks, weight and
cycle remain raw observations; the interval is independent of those fields.

The `engineering-source-text-key-interval-v1` observation records whole source
SHA, sequence/text-key block spans and digests, exact request time words, raw
clock fields and the selected text-key declaration. Entries preserve physical
key ordinal, binary32 time bits, exact string-table index and raw string bytes.
No sorting or duplicate removal occurs. Nonmonotonic source order, repeated
times, authored empty strings/arrays and an empty query result are supported.
Binary32 source times are promoted exactly for comparison with binary64 request
bounds; +0 and -0 compare equal while their source/request words remain distinct.

Missing sequence/text link, wrong link class, invalid source/string references,
malformed input and nonfinite request bounds refuse. The existing decoder rejects
nonfinite source floats anywhere in its decoded source blocks with exact byte
offsets, including unselected blocks. Every key of the selected linked text block
must additionally have an authored string, even outside the requested interval.
Unlinked text blocks are never merged or substituted for the selected link.

The two-pass query validates and counts entries/raw copied bytes before allocating
output vectors or strings. Per-entry storage errors identify exact text-key block,
key ordinal and, for string copies, string index. Default extra output is 4 MiB,
work one million catalogue/key/copy units, source arrays 32 MiB and blocks 16,384,
with source+output charged storage capped together at 36 MiB. The existing source
decoder's check/input limits also remain active. The usage receipt separates
decoded source and output logical storage; allocator overhead, vector capacity
and decoder scratch excluded by the existing source contract are not process
memory claims. Failure emits no partial observation.

The headless consumer is:

```text
fallout nif-animation INPUT --markers-request REQUEST.json --output RECEIPT.json
```

Strict schema1 JSON requires `expected_sha256` as 32 byte integers, `sequence`,
`source_start` and `source_end`. Request JSON is capped at 64 KiB and source at
64 MiB; output must be outside both directories. Source-report/sampling options
conflict with the selected query. Unsupported evaluation returns `evaluation:
null`, an exact error and nonzero exit. Existing source-report schemas remain
unchanged. `tools/nif-animation-oracle/check_markers.py` checks a second authored
source and the preserved native-decoded first-person `1gtaim.kf` source. Marker
strings are source observations only: no weapon damage, sound dispatch, event
cursor, repeat/cycle, transition timing or measured retail playback is established.

Focused validation passes 31 data tests (eight new interval cases), 38 CLI tests,
affected all-target Clippy with warnings denied, formatting and the CLI build.
The frozen consumer passes four second-source interval observations and ten
intended refusals, plus three native comparisons against original sequence0's
text-key block64 (whole selected interval, exact zero and exact last time).
Raw key/string ordinals, bytes, time words and both source-block spans/hashes
match the independent native projection; source schemas1..6 remain byte-identical
to the preceding frozen binary. Initial tests incorrectly expected nonfinite
floats to reach the query and permitted a nonfinite unlinked block; the decoder's
stricter existing refusals were retained, with corrected exact-offset expectations
passing. One prematurely queued validation was cancelled before any child build;
its empty log/cancellation receipt and all first-run failures stay preserved.
Evidence and frozen executable remain in ignored `local/v3-asset-19`.
