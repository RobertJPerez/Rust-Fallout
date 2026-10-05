# Preparation checkpoint 11: texture-set path census

Date: 2026-10-04

This checkpoint adds `trace-texture-sets`, a bounded source-field and asset-name
census for Skyrim `TXST` records. The adapter reuses the pinned TES4 selected
record/subrecord visitor, source hashing, `AssetPath` normalizer and BSA 105
archive backend. It does not copy record framing, archive decompression or path
normalization logic. The eight texture-field roles follow the pinned Skyrim
definitions and `SkyrimTexture` schema recorded in
[`texture-set-assets.md`](../docs/texture-set-assets.md).

The read-only scan measured Steam build 24914197 / `SkyrimSE.exe` 1.7.104.0
(SHA-256 `846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`).
It scanned the ten plugin files and indexed all 23 BSA 105 archives under the
installed `Data` directory. The install contains four included Creation
plugin/archive pairs; this is not the full paid Anniversary content set.

The ten physical plugins contain 790 `TXST` records, 4,606 visited TXST
subrecords and 2,182 non-empty texture fields. Those fields normalize to 1,149
unique texture paths. A `TXST` field is
texture-root-relative in this adapter, so `actors/...` is checked as the single
Data-root candidate `textures/actors/...`. This mapping is explicit in each
output reference as `lookup.data_asset_path`; the plugin field value remains
separate. The trace found 2,153 field references with a BSA name candidate, zero
loose-file candidates, and 29 references with no candidate in this install.
There were no malformed paths, duplicate slots, unknown subrecords, archive
index failures, hash-only archive entries, invalid archive paths or duplicate
archive candidates. The command exits 2 because unmatched candidates are
findings; the JSONL still has a completion trailer and all source/archive
fingerprints.

| Plugin | `TXST` records | Texture fields | Unmatched fields |
| --- | ---: | ---: | ---: |
| `_ResourcePack.esl` | 34 | 72 | 0 |
| `ccBGSSSE001-Fish.esm` | 28 | 61 | 2 |
| `ccBGSSSE025-AdvDSGS.esm` | 13 | 39 | 2 |
| `ccBGSSSE037-Curios.esl` | 3 | 6 | 1 |
| `ccQDRSSE001-SurvivalMode.esl` | 0 | 0 | 0 |
| `Dawnguard.esm` | 53 | 144 | 0 |
| `Dragonborn.esm` | 59 | 127 | 0 |
| `HearthFires.esm` | 21 | 42 | 0 |
| `Skyrim.esm` | 572 | 1,677 | 24 |
| `Update.esm` | 7 | 14 | 0 |

The unmatched entries remain visible with plugin, FormID, slot, source offsets,
raw field evidence and exact candidate path in ignored local JSONL. This is not
proof that they are invalid: the scan does not identify optional-content
ownership, complete AE coverage, archive precedence, texture payload validity,
material/decal semantics, plugin winners or runtime use. `OBND`, `DODT`, `DNAM`
and unknown subrecords remain opaque bounded bytes with full-field hashes.

Collection is limited to 100,000 `TXST` records and 100,000 subrecords across
the supplied plugins; retained raw path/field evidence is capped at 1,024 bytes
per field. All 65 Rust tests pass, formatting passes, Clippy passes with warnings denied,
and the one-job build passes. Retail files remain unchanged. The JSONL evidence
hash and source manifest receipt are in
[`preparation-11.json`](preparation-11.json); the command scope and limitations
are documented in [`texture-set-assets.md`](../docs/texture-set-assets.md).
