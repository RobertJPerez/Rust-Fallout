# Preparation checkpoint 16: CELL grid coordinates

Date: 2026-10-04

This checkpoint adds a bounded structural reader for CELL XCLC fields to
trace-landscape-links. It visits CELL subrecord framing through the pinned
shared record reader and decodes only the documented signed X/Y fields and
optional tail. Full payload lengths and hashes remain available with bounded
raw previews. Unsupported lengths, repeated XCLC fields, and cells without
XCLC are explicit findings. It does not convert cell-grid integers to world
units or infer streaming, persistence, or gameplay behavior.

Across the ten supplied plugins, 76,356 of 77,337 physical CELL records have
one 12-byte XCLC field; 981 have none. There are no repeated or unsupported
XCLC payloads in this corpus. Each observed field retains signed coordinates,
a flags byte, and three reserved bytes. The raw flags values are 0 (76,260),
1 (2), 3 (5), 4 (3), 5 (2), 12 (5), 13 (4), 14 (1), and 15 (74). Across all
supported fields, 34,565 distinct coordinate pairs occur; the observed X
range is -98 through 127 and Y range is -94 through 127. These ranges describe
only this installed plugin corpus.

The scan also profiles 400,969 CELL subrecords across 25 tags while leaving
non-XCLC payloads opaque. Examples include 33,093 MHDT fields of 1,028 bytes,
981 XCLL fields (980 of 92 bytes and one of 64 bytes), and XCLR fields ranging
from 4 to 52 bytes. The complete tag/length distributions remain in the
machine-readable trace; they are evidence for selecting future adapters, not
field decodings.

The LAND ancestry trace contains 54,079 cell labels resolving to 52,181
distinct containing-cell source keys. Every referenced key has physical CELL
candidate rows with one supported XCLC coordinate each, and all candidates
for a key agree. This covers all 54,079 LAND rows and all 52,181 distinct
world/cell source-key pairs. Candidate rows remain separate; no active load
order or winning override is inferred. The 981 CELL records without XCLC
are not candidates for the LAND-referenced keys in this scan.

The existing landscape chain still has 101 layer references without a scanned
LTEX candidate: Skyrim.esm LTEX local ID 0x00000844, comprising 74 BTXT
and 27 ATXT fields. The command completed with findings status 2 for this
pre-existing gap. It reported zero malformed plugin-reader inputs and all ten
source plugin fingerprints matched the current install census.

The installation uses Steam build 24914197 and executable version 1.7.104.0.
The ten-plugin, 23-archive profile has four installed Creation pairs and does
not establish the complete paid Anniversary content set. The current census
has eight blocking findings and zero reader failures. Original game files
remained read-only.

All 68 Rust tests pass. Formatting, Clippy with warnings denied, and the
one-job build pass. The installed trace is
local/landscape-grid-16b.jsonl (344,740,915 bytes,
SHA-256 0ea38ef95f59d0e55b934ddcf4c22b20a24a2adb9a1c63921b0df0817181ab96).
The machine-readable summary is preparation-16.json; the source manifest is
source-manifest-16.json. See the landscape and cell-context contract in
docs/landscape-links.md. No renderer or gameplay acceptance is claimed.
