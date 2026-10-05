# Preparation checkpoint 17: CELL water-height bits

Date: 2026-10-04

This checkpoint extends the bounded CELL adapter to retain XCLW. The pinned
Skyrim field definition is a four-byte IEEE 754 single-precision water-height
value. Each field preserves its raw bits, hexadecimal bit pattern, full hash,
and bounded bytes. Finite values also receive a numeric field; infinities and
NaNs keep their bits and class but produce no JSON number. Unsupported field
lengths remain visible. The scanner does not synthesize a WRLD default or claim
runtime water selection.

All 77,337 physical CELL records in the ten supplied plugins have exactly one
four-byte XCLW field. No field is missing, repeated, malformed, or classified
as IEEE non-finite. The corpus contains 124 distinct raw bit patterns.
0x7F7FFFFF occurs 74,638 times and 0xFF7FFFFF occurs 28 times. They are kept as
raw values without assigning default, selection, or gameplay meaning.

LAND group labels identify 52,181 distinct containing-cell source keys across
54,079 LAND rows. Every referenced key has physical CELL candidates with one
supported XCLW field each. Candidate raw bits agree for 52,148 keys; 33 keys
have differing candidate bit patterns, affecting 54 LAND rows. The trace
retains every physical candidate. An ignored local ledger records each
disagreement with candidate plugin and record offsets for later explicit
load-order work; no override is selected.

The differing candidates come from these physical plugin groups:

| Candidate plugins | Cell keys | LAND rows |
| --- | ---: | ---: |
| Skyrim.esm + Update.esm | 10 | 18 |
| ccBGSSSE001-Fish.esm + Skyrim.esm | 2 | 2 |
| ccBGSSSE001-Fish.esm + Skyrim.esm + Update.esm | 6 | 11 |
| ccBGSSSE001-Fish.esm + HearthFires.esm + Skyrim.esm + Update.esm | 1 | 2 |
| Dawnguard.esm + Skyrim.esm + Update.esm | 2 | 3 |
| Dragonborn.esm + Skyrim.esm | 3 | 3 |
| Dragonborn.esm + Skyrim.esm + Update.esm | 5 | 10 |
| HearthFires.esm + Skyrim.esm + Update.esm | 4 | 5 |

The existing landscape chain still has 101 layer references without a scanned
LTEX candidate: Skyrim.esm LTEX local ID 0x00000844, comprising 74 BTXT and
27 ATXT fields. The command completed with findings status 2 for this gap.
All ten source plugin fingerprints match the current install census.

The installation uses Steam build 24914197 and executable version 1.7.104.0.
The ten-plugin, 23-archive profile has four installed Creation pairs and does
not establish the complete paid Anniversary content set. The current census
has eight blocking findings and zero reader failures. Original game files
remained read-only.

All 68 Rust tests pass. Formatting, Clippy with warnings denied, and the
one-job build pass. The installed trace is
local/landscape-water-height-17a.jsonl (375,487,663 bytes,
SHA-256 3c9b298a588aeb5f68e0e433779a094e769e66d25aa374a94653c7b774e0b6b3).
Its conflict ledger is
local/cell-water-candidate-conflicts-17a.json (30,853 bytes, SHA-256
5d84f17c64f925b98d6174b4d80b8fe8d3d9051fe38ca9d70d6a5c3ca608b8b9).
The machine-readable summary is preparation-17.json; the source manifest is
source-manifest-17.json. See the landscape and cell-context contract in
docs/landscape-links.md. No renderer or gameplay acceptance is claimed.
