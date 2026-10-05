# Preparation checkpoint 12: landscape texture links

Date: 2026-10-04

This checkpoint adds `trace-landscape-links`, a bounded Skyrim source graph for
terrain texture layers. It parses `LAND` `BTXT`/`ATXT` eight-byte layer headers,
then follows `LTEX` `TNAM` links to `TXST`, `MNAM` links to `MATT`, and repeated
`GNAM` links to `GRAS`. Layouts are based on the pinned Skyrim definitions already
recorded in `sources.lock.json`. Record framing, decompression, identity
normalization and source hashing continue to use the existing shared data
library.

The JSONL report emits each physical record once. An edge retains its raw FormID,
target kind, resolved source key where possible, candidate count, and status. The
target records are joinable by kind and source key, so all scanned physical
overrides remain available without selecting a winner. Full FormIDs resolve
through each source file's declared masters; light-plugin-owned record IDs use
the checked HEDR rule; cross-file FE links remain unresolved without a profile.
The report does not decode terrain geometry, heights, normals, alpha weights,
materials, texture pixels, or runtime behavior. Its `TXST` rows join by physical
plugin and FormID to the texture paths already exported by checkpoint 11.

The current read-only install census again measured Steam build 24914197,
`SkyrimSE.exe` 1.7.104.0, and executable SHA-256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`. The ten
plugins in the landscape report have the same byte counts and SHA-256 values as
the current census. It indexed 23 archives and found four installed Creation
plugin/archive pairs; this is not the complete paid Anniversary Edition corpus.
The broader census completed with zero reader failures and retained its eight
existing blocking findings, separate from this landscape scan.

Across the ten plugins, the trace selected 55,139 physical records: 54,079
`LAND`, 130 `LTEX`, 790 `TXST`, 80 `MATT`, and 60 `GRAS`. It visited 526,715
`LAND`/`LTEX` subrecords and found 49,345 base layers, 152,844 alpha layers,
129 `TNAM` fields, 130 `MNAM` fields, and 60 `GNAM` fields. Of 187,175 non-null
links resolved through ordinary master selectors, 187,074 found at least one
physical target among the scanned files. The 101 without a scanned target all
point to `Skyrim.esm` LTEX local ID `0x00000844`: 74 `BTXT` and 27 `ATXT`
references. They remain visible as unresolved corpus links, not as proof that
the ID is invalid or that optional content is complete. There are also 15,333
null links.

No layer headers or FormID payloads had malformed sizes; there were no duplicate
layer slots, out-of-range quadrants, invalid master indices, or FE links in this
installed source set. A byte in the eight-byte layer header is nonzero in
111,450 fields; the trace retains it without assigning meaning or treating it
as a failure. Unknown subrecords stay opaque with full-field hashes and bounded
32-byte previews. The completed command exits 2 because 101 links have no
scanned target candidate, and writes a completion trailer.

The landscape evidence is `local/landscape-links-12e.jsonl` (231,844,755 bytes,
SHA-256 `5d430cd815f4ee4ff15299936d19d414b47e95e7fb09dd9d757d8e429b6363de`).
The current install census is `local/census-20261004T222237Z-6a88a6f8.json`;
its receipt is `local/install-20261004T222237Z-6a88a6f8.json`. Checkpoint 11's
`local/texture-sets-11c.jsonl` has matching fingerprints for all ten current
plugins and indexes the same 23 BSA names. These local files are ignored and
contain no distributed retail assets.

All 68 Rust tests pass. Formatting passes, Clippy passes with warnings denied,
and the one-job build passes. See the
[landscape link contract](../docs/landscape-links.md). No gameplay or renderer
acceptance is claimed.
