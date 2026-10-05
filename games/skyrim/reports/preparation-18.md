# Preparation checkpoint 18: CELL lighting payload profile

Date: 2026-10-04

This checkpoint attaches CELL XCLL lighting payload evidence to each physical
CELL record. The trace retains exact payload length, full SHA-256, and a bounded
32-byte preview. It identifies the two sizes observed in this install, 64 and
92 bytes, but decodes no lighting members. Other lengths would stay visible as
unprofiled; none occurred in the supplied plugins.

The ten-plugin scan contains 981 XCLL fields: 980 are 92 bytes and one is 64
bytes. The sole 64-byte field is in Skyrim.esm. XCLL counts by plugin are
Skyrim.esm 590, Dawnguard.esm 114, Update.esm 91, Dragonborn.esm 88,
ccBGSSSE001-Fish.esm 46, HearthFires.esm 46, ccBGSSSE025-AdvDSGS.esm 5, and
ccQDRSSE001-SurvivalMode.esl 1. The installed profile still contains only
four Creation pairs and does not establish the complete paid Anniversary
content set.

The existing LAND ancestry trace contains 54,079 rows and 52,181 distinct
containing-cell source keys. None of their physical CELL candidates has an
XCLL field in these supplied plugins. This only describes the scanned physical
sources; it does not identify which cells are interior or assign lighting
behavior. XCLL therefore remains separate from the LAND terrain-cell mapping
for this profile.

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
local/cell-lighting-18a.jsonl (379,239,565 bytes,
SHA-256 c3a2285da9656ead44ecba5cc837ca46b09e63ec96e0162b0d6f67974d5743c9).
The machine-readable summary is preparation-18.json; the source manifest is
source-manifest-18.json. See the landscape and cell-context contract in
docs/landscape-links.md. No renderer or gameplay acceptance is claimed.