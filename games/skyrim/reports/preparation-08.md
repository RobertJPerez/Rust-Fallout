# Preparation checkpoint 08: SSE NIF outer-table index

Date: 2026-10-04

This checkpoint adds a bounded Skyrim-specific index of SSE NIF outer tables.
`nif-index` accepts only the exact text/binary version 20.2.0.7, endian marker 1,
User Version 12, and stream version 100 tuple. It reports the block type table,
raw and masked type indices, the uninterpreted upper index bit, exact byte spans,
the string table, groups, roots, and footer accounting. The payload bytes are
never decoded. It rejects out-of-range type/root indices, truncated fields,
oversized counts or strings, spans that exceed the member, and trailing bytes.

The parser was exercised against six read-only members in the installed SE
archives. All six have the target tuple, contiguous block spans that end exactly
at the parsed footer, and no trailing bytes. They cover the base meshes plus all
four Creation archives present in this installation:

| Archive | Member | Bytes | Blocks | Types | Strings | SHA-256 prefix |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| `Skyrim - Meshes0.bsa` | `meshes\\actors\\character\\character assets\\skeleton.nif` | 24,207 | 268 | 22 | 166 | `139827f3ff6d` |
| `Skyrim - Meshes1.bsa` | `meshes\\LoadScreenArt\\LoadScreenMRaltar01.nif` | 39,133 | 7 | 4 | 3 | `4f0ad3a5c7a9` |
| `ccBGSSSE001-Fish.bsa` | `meshes\\creationclub\\bgssse001\\critters\\fish\\fishflameangelfish\\fishflameangelfish01inv.nif` | 15,453 | 10 | 10 | 4 | `0a7f6f8efd54` |
| `ccBGSSSE025-AdvDSGS.bsa` | `meshes\\creationclub\\bgssse025\\loadscreenart\\loadscreendarkseducerbattleaxe.nif` | 85,025 | 4 | 4 | 2 | `9dd5c690f94b` |
| `ccBGSSSE037-Curios.bsa` | `meshes\\creationclub\\bgssse037\\clutter\\heartoforder\\heartoforder.nif` | 8,554 | 14 | 12 | 4 | `3ab61f0e4ea3` |
| `ccQDRSSE001-SurvivalMode.bsa` | `meshes\\survival\\maginvhungerpenaltyspellart.nif` | 23,716 | 93 | 39 | 40 | `54bed585c199` |

For the base skeleton sample, blocks begin at byte 291 and all 268 adjacent spans
end exactly at the footer; the parsed file ends at byte 24,207. The remaining
five files have the same verified span/footer invariant. Full member and report
hashes are in the ignored local JSON evidence indexed by `preparation-08.json`.

Both base mesh path sets were completely enumerated using offset pages, without
decompressing members during the listing phase. `Skyrim - Meshes0.bsa` contains
18,862 NIF paths across 37 pages; `Skyrim - Meshes1.bsa` contains 3,185 across
7 pages. Page concatenation reproduces each archive count exactly, with no
duplicate normalized NIF paths, hash-only entries or invalid paths.

All 3,185 `Meshes1` assets passed outer-table indexing. In `Meshes0`, 18,861 of
18,862 members passed; one legacy stream tuple remains a visible finding:

| Member | Bytes | SHA-256 | Header tuple | Result |
| --- | ---: | --- | --- | --- |
| `meshes\\creationclub\\_shared\\dungeons\\ayleidruins\\interior\\triggers\\artrigpressureplate01.nif` | 20,755 | `b1a96a9c034e90ac4ea81fdaf44983ebfaea33ce368648dae4555a5b8c45ba9e` | 20.2.0.7 / user 12 / stream 83 | Header retained; not passed to the stream-100 indexer |

That member's path and archive presence do not establish which runtime consumes
it or Creation ownership. Its compatibility remains unknown. The other 18,861
`Meshes0` members use the admitted tuple and pass exact block/footer accounting.

The same check covered every NIF path in the four Creation archives available in
this install:

| Archive | NIF paths | Indexed | Findings |
| --- | ---: | ---: | ---: |
| `ccBGSSSE001-Fish.bsa` | 231 | 231 | 0 |
| `ccBGSSSE025-AdvDSGS.bsa` | 266 | 266 | 0 |
| `ccBGSSSE037-Curios.bsa` | 65 | 65 | 0 |
| `ccQDRSSE001-SurvivalMode.bsa` | 4 | 4 | 0 |
| **Total** | **566** | **566** | **0** |

All four Creation path indexes also report zero duplicate normalized paths,
hash-only entries or invalid paths. They are not truncated. All 566 NIFs in the
four installed Creation archives pass the stream-100 indexer. Across the two
base archives and four available Creation archives, the single stream-83 file
is the only unsupported tuple or outer-table finding in this inventory. The
Creation set remains an incomplete paid AE corpus.

The indexed base archives contain 758,687 block instances across 143 observed
type names and 2,207,492,842 decoded bytes. The one stream-83 member is excluded
from these counts. The four Creation archives contain 22,853 block instances
across 89 names and 72,444,076 decoded bytes. A case-sensitive comparison found
all 89 Creation-sample type names in the indexed base set; 54 additional names
were observed in base meshes. This compares on-disk names only and says nothing
about payload semantics or content ownership. Full per-file counts and the
comparison are retained in ignored local evidence.

These are bounded format-level checks over an installed SE sample. They do not
establish all-mesh coverage, paid Anniversary Edition content coverage, block
payload semantics, or runtime compatibility. The installed Creation/plugin set
remains the incomplete profile recorded in checkpoint 07; absence of paid
Creation files is not treated as proof of ownership or a complete AE inventory.

The pinned shared `fallout-data::nif` reader remains unchanged and admits the
observed NV user-11 stream family only. Skyrim uses its own exact-tuple adapter
and the existing shared BSA backend/member path services; no NV block decoder,
record framing, decompressor, or path normalizer is duplicated. A future shared
versioned table API would require a separate dependency review. The pinned
NifTools schema reference is used only for field/version evidence. Bethesda
20.2.0.7 inheritance differences mean the outer table is not evidence that NV or
FO4 payload decoders can process Skyrim blocks.

Validation uses synthetic distributable NIF/BSA fixtures: table and span
accounting, every truncation boundary, unsupported tuples, invalid block types
and roots, and trailing data. The installed members were read from original
archives without extracting or modifying game content. Full tests, formatting,
Clippy with warnings denied, and a single-job build are recorded in
`preparation-08.json`; no gameplay scenario is accepted.

Machine-readable report: [`preparation-08.json`](preparation-08.json). The local
probe JSON is ignored under `local/` and its path, size, and hash are recorded
there. The source manifest lists the exact source/document files included in
this checkpoint.
