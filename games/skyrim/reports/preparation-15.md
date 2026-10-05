# Preparation checkpoint 15: LAND cell and world context

Date: 2026-10-04

This checkpoint extends `trace-landscape-links` to emit physical `CELL` and
`WRLD` identities alongside the already retained LAND ancestor-group path. The
shared Skyrim trace helper extracts kind 6 and kind 1 labels as raw IDs; the
existing source-key resolver maps them through each referring plugin's declared
masters. Physical candidate counts are kept per LAND row. The command does not
read CELL or WRLD bodies, infer coordinates, or choose an override.

Across the ten supplied plugins, all 54,079 LAND rows have both a kind 6 cell
label and kind 1 world label. Both labels resolve to source keys, and every one
has at least one physical candidate in the scanned plugins. Cell candidate
counts are one for 49,483 rows, two for 3,571, three for 861, four for 151,
five for 12, and six for one row. World candidate counts range from one to
seven; the exact distribution is preserved in the JSON report. There are
52,181 distinct world/cell source-key pairs represented by the LAND rows.
Multiple candidates remain separate because no active winning load order was
provided.

The scan selected 132,570 physical records: 54,079 LAND, 77,337 CELL, 94 WRLD,
130 LTEX, 790 TXST, 80 MATT, and 60 GRAS. The shared reader supplied the same
six-frame LAND group path for every row. In this corpus the kind 9 raw label
equals the kind 6 raw label on all LAND rows; this observation is retained
without assigning another semantic role to that group.

The existing landscape links still contain 101 LTEX references without a
candidate in the supplied files: Skyrim.esm LTEX local ID `0x00000844`, with
74 BTXT and 27 ATXT fields. The command writes a completion trailer and exits
2 for this finding. No light-label join required an active profile in this
scan, but cross-file FE references remain unresolved by design when present.

All ten plugin fingerprints match the current install census. The measured
Steam build is 24914197 with runtime 1.7.104.0. The ten-plugin, 23-archive
install contains four Creation pairs and is not the complete paid Anniversary
content set. The census reports eight blocking findings and zero reader
failures. Original game files were read-only.

All 68 tests pass, including a synthetic master-list join for LAND, CELL, and
WRLD. Formatting passes, Clippy passes with warnings denied, and the one-job
build passes. The installed report is
`local/landscape-cell-context-15b.jsonl` (318,210,635 bytes, SHA-256
`fb8c18faeed0ec2d152c3a18cd830683665c4e0de6e46077f2008d8a809ef2ce`). The
machine-readable summary is [preparation-15.json](preparation-15.json); the
source manifest is [source-manifest-15.json](source-manifest-15.json). See the
[landscape and cell-context contract](../docs/landscape-links.md). No renderer
or gameplay acceptance is claimed.
