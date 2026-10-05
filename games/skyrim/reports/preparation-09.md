# Preparation checkpoint 09: generic model source fields

Date: 2026-10-04

This checkpoint adds `export-generic-models`, a Skyrim-specific source-field
exporter for the 39 non-`STAT` record kinds whose pinned schema declares the
generic model structure. It reuses the pinned `fallout-data` plugin visitor,
source hashing and `AssetPath` normalizer. The existing `STAT` exporter remains
responsible for `MNAM`'s four fixed distant-LOD slots. No record framing,
decompression or path-normalization implementation was duplicated.

The installed Steam profile remains build 24914197 / runtime 1.7.104.0. The
source scan covered the ten installed plugin files and found 19,636 records in
the selected record kinds. Their 15,079 `MODL` fields all normalized, with zero
malformed model paths:

| Plugin | Selected records | Record kinds seen | `MODL` fields | Path findings |
| --- | ---: | ---: | ---: | ---: |
| `_ResourcePack.esl` | 89 | 10 | 84 | 0 |
| `ccBGSSSE001-Fish.esm` | 429 | 18 | 310 | 0 |
| `ccBGSSSE025-AdvDSGS.esm` | 180 | 20 | 165 | 0 |
| `ccBGSSSE037-Curios.esl` | 73 | 10 | 70 | 0 |
| `ccQDRSSE001-SurvivalMode.esl` | 139 | 4 | 129 | 0 |
| `Dawnguard.esm` | 1,376 | 35 | 1,037 | 0 |
| `Dragonborn.esm` | 2,294 | 34 | 1,922 | 0 |
| `HearthFires.esm` | 788 | 17 | 693 | 0 |
| `Skyrim.esm` | 13,759 | 38 | 10,259 | 0 |
| `Update.esm` | 509 | 25 | 410 | 0 |
| **Total** | **19,636** | — | **15,079** | **0** |

The wider source-field inventory locates the unsupported stream-83 Ayleid NIF
in `Update.esm` `ACTI` record `0x01003206` (file-local), editor ID
`ccBGS_ARTrigPressurePlate01`, at source offset 808,329. Its model field is
`CreationClub\_Shared\Dungeons\AyleidRuins\Interior\Triggers\ARTrigPressurePlate01.NIF`.
The separately rooted `meshes/` candidate matches the member in
`Skyrim - Meshes0.bsa`; the 20,755-byte member hash and legacy header tuple are
preserved in ignored local evidence. None of the ten installed `STAT` `MODL`
fields exactly names this file. The four other unmatched `Update.esm` Ayleid
`STAT` paths have no matching generic-model field in the ten-plugin scan.

This establishes a source field and an archive candidate for the legacy NIF. It
does not establish that the record is active or placed, determine archive
precedence, establish Creation ownership, or show that the installed runtime
consumes stream 83. That payload remains unsupported. The four installed
Creation plugin/archive pairs also do not make this a complete paid Anniversary
Edition corpus.

Synthetic tests cover a schema-allowed `ACTI`, ignore an unsupported `CELL` and
the separately handled `STAT` kind, and retain an unterminated `MODL` as a path
finding. Validation passed 59 Rust tests, formatting, Clippy with warnings
denied, and a single-job build. Installed game files remained read-only.

Ignored JSONL plugin exports and the targeted NIF trace are listed in
[`preparation-09.json`](preparation-09.json). The exact Rust source, tests,
configuration and project documentation for this checkpoint are hashed by the
[source manifest](source-manifest-09.json). The full command and interpretation
limits are in [`docs/generic-model-assets.md`](../docs/generic-model-assets.md).
