# NV animation source framing

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
