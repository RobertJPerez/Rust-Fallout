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

Next: partition palettes, maps, weights and topology in independently verified
NV branches (ASSET-02); then graph/root/bone binding and its evaluation contract
(ASSET-03). Skin-root membership, evaluated poses, transforms composed for
skinning, normalization rules, dismemberment behavior, animation, attachments
and retail presentation/gameplay remain unverified.
