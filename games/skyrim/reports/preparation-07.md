# Skyrim preparation checkpoint 07

This checkpoint adds a Skyrim-owned `STAT` model-field adapter and a multi-plugin
source-to-asset index trace. `export-stat-models` frames selected `STAT` record
bodies through the pinned shared TES4 visitor. It preserves source FormIDs,
physical record offsets, decoded-record field offsets, `EDID` bytes/text, raw
path bytes, field hashes and a completion trailer. The adapter extracts `MODL`
paths, hashes opaque `MODT` and `MODS` payloads, and separates the four 260-byte
`MNAM` slots into LOD levels. Unknown metadata and malformed paths remain
visible; no NIF payload or runtime asset behavior is inferred.

`trace-stat-assets` accepts repeated `--plugin` inputs, indexes the selected
directory's BSA set once, and checks both exact source paths and separate
`meshes/`-rooted candidates against loose files and all BSA 105 names-only
indexes. It hashes each archive, preserves every matching archive/member, and
does not choose plugin overrides, archive precedence or runtime winners. It does
not decompress BSA payloads. The candidate rule is corpus evidence, not a runtime
policy.

The read-only installed Skyrim SE build is 24914197 / runtime 1.7.104.0. The
installed `Skyrim.esm` is 249,752,131 bytes with SHA-256
`e198c3b85e5e48e0c92a6580d8f66e644256b68d812ee32b61735cf9b753df73`. The scan
covered ten installed plugin sources:

| Plugin | `STAT` | `MODL` | `MNAM` | LOD strings | Source paths | Archive matches | Unmatched |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `_ResourcePack.esl` | 223 | 223 | 0 | 0 | 223 | 223 | 0 |
| `ccBGSSSE001-Fish.esm` | 55 | 55 | 0 | 0 | 55 | 55 | 0 |
| `ccBGSSSE025-AdvDSGS.esm` | 45 | 45 | 0 | 0 | 45 | 45 | 0 |
| `ccBGSSSE037-Curios.esl` | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `ccQDRSSE001-SurvivalMode.esl` | 3 | 3 | 0 | 0 | 3 | 3 | 0 |
| `Dawnguard.esm` | 930 | 930 | 106 | 352 | 1,282 | 1,276 | 6 |
| `Dragonborn.esm` | 1,085 | 1,085 | 155 | 453 | 1,538 | 1,537 | 1 |
| `HearthFires.esm` | 186 | 186 | 0 | 0 | 186 | 186 | 0 |
| `Skyrim.esm` | 9,720 | 9,712 | 824 | 2,256 | 11,968 | 11,935 | 33 |
| `Update.esm` | 379 | 379 | 11 | 24 | 403 | 399 | 4 |
| **Total** | **12,626** | **12,618** | **1,096** | **3,085** | **15,703** | **15,659** | **44** |

All 12,626 bounded editor IDs were retained; malformed editor IDs, model paths,
and LOD shapes were zero. Eight records have no `MODL`. The scan hashed 10,565
opaque `MODT`/`MODS` payloads in `Skyrim.esm`; these payloads are not decoded.

The trace emitted 15,703 source path references and 19,886 unique lookup
candidates. It indexed all 23 installed BSA files; all were v105 and none had
index failures, hash-only paths, invalid paths or duplicate normalized candidate
entries. All 15,659 archive matches came from the separate `meshes/`-rooted
candidate; no exact field-path candidate matched. A candidate was found in an
archive for 15,659 references; 44 had no
loose-file or archive match in this installed content set. These are source
references, not a claim that all paths name NIF files. Four unmatched
`Update.esm` paths are under `CreationClub\\_Shared\\Dungeons\\AyleidRuins`; the
currently installed files do not supply them. That observation does not establish
content ownership or completeness of the paid Anniversary Edition corpus. Thirty
source references have members in multiple archives. All matches and overlaps
remain visible without an ordering decision.

The 8 records without a `MODL` field, 44 unmatched references and 30
multi-archive references remain findings. Source-addressed TSVs of the unmatched
records and cross-archive overlaps are retained under ignored `local/` for
follow-up. Complete per-record JSONL exports for all ten plugin sources also
preserve the `MODT`/`MODS` payload hashes and source offsets. No active load
order, winning override, archive precedence or runtime use is asserted.

For a positive cross-check, source field
`LoadScreenArt\LoadScreenMRaltar01.nif` has the archive candidate
`meshes/LoadScreenArt/LoadScreenMRaltar01.nif` in `Skyrim - Meshes1.bsa`. The
bounded NIF probe reads 39,133 bytes and identifies the 20.2.0.7 / User Version
12 / stream 100 header tuple. This confirms one source-to-index mapping and a
header only. No block, mesh, material, skinning, animation, collision or runtime
support is accepted. The paid Anniversary corpus remains incomplete.

The New Vegas checkout was rechecked read-only at clean main revision
`d35da72af8af1cb5f0ea062862e3d35d8f5f22c4`; the frozen shared dependency remains
checkpoint 45, revision `9265ef63f714cdf01ff02028640a582be9639c7a`. The Fallout 4
checkout at `69f12091aa8b1b22530a8e203bdfbafc71c72a71` has active dirty work and
was left untouched. The Skyrim implementation adds no NIF payload decoder and
no PEX decoder.

The schema source is pinned TES5Edit commit
`9fb016884bec138ea6c7b872cec831537d464c3e`; the implementation uses the existing
`fallout-data` checkpoint-45 plugin visitor and path normalizer plus the pinned
`dream_archive` BSA reader. Current NV NIF geometry/material/skin/collision
decoders were not used for Skyrim. Fallout 4's PEX parser remains the bytecode
owner; no Skyrim PEX decoder or VM was added.

The installed NIF sample's `20.2.0.7`/user-12/stream-100 tuple is outside the
pinned NV `fallout-data::nif` indexer's user-11 stream gate. Skyrim remains at
header-only evidence; its block table and Bethesda 20.2 geometry inheritance are
not decoded. The [Skyrim NIF handoff](../docs/nif-header-probe.md) records the
versioned outer-table fields and the boundary before payload parsing.

Verification passed: all 52 Rust tests, formatting check, warnings-denied
Clippy, and the locked single-job build. Synthetic tests cover raw/normalized
model paths, malformed paths, fixed LOD slots, incomplete JSONL output,
meshes-rooted BSA matches and preservation of archive candidates. Original game
files remain read-only; all detailed retail output is under ignored `local/`.

See the [static model contract](../docs/static-model-assets.md),
[reuse boundary](../docs/shared-integration.md), and
[remaining gates](../NEXT_STEPS.md). The machine record
[preparation-07.json](preparation-07.json) points to a hashed source manifest
and hashed local evidence files.
