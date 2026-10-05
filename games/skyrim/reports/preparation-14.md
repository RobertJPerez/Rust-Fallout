# Preparation checkpoint 14: LAND field cardinalities

Date: 2026-10-04

This checkpoint extends `trace-landscape-links` with a per-record occurrence
histogram for each LAND subrecord tag. The trailer records how many physical
LAND records contain zero, one, or multiple instances of each observed tag,
alongside the exact payload-size profile. This is a bounded Skyrim-specific
preparation step for a later terrain adapter; it does not decode vertex data or
assign field meaning.

The ten supplied plugins contain 54,079 LAND records. `DATA` occurs once in
each. The 53,896 records with `VHGT` also have `VNML`; both fields are absent
together in the other 183 records. `VCLR` occurs in 9,243 records: 9,239 have
`VHGT` too, four have `VCLR` without it, 44,657 have `VHGT` without `VCLR`,
and 179 have neither. These are physical field co-occurrence checks, not terrain
validity judgments.

`BTXT` and `ATXT` each have eight-byte payloads in this corpus. `VTXT` appears
152,661 times across the 54,079 records and has 289 observed payload lengths,
from 8 through 2,312 bytes in eight-byte steps. Its count is not one-to-one
with `ATXT`: the counts match on 53,917 records; `ATXT` occurs one extra time
on 143, two extra times on 17, and three extra times on two records. All
payloads remain opaque, so these relationships are useful parser constraints,
not decoded array semantics.

The linked graph remains as before: 187,175 full-ID source-key resolutions,
187,074 matching physical target candidates, and 101 layer references without
a scanned LTEX candidate. All 101 point to Skyrim.esm LTEX local ID `0x00000844`
(74 BTXT and 27 ATXT). The command writes its completion trailer and exits 2
for this finding. It still does not choose winners, decode terrain, infer
rendering behavior, or claim gameplay acceptance.

All ten plugin fingerprints match the current install census. The measured
Steam build is 24914197 with runtime 1.7.104.0; the ten-plugin, 23-archive
install has four Creation pairs and is not the full paid Anniversary content
set. The census reports eight blocking findings and no reader failures. Retail
files were read-only; generated corpus output remains under ignored `local/`.

The Rust suite passes all 68 tests, formatting passes, Clippy passes with
warnings denied, and the one-job build passes. The synthetic LAND fixture
checks absent and repeated VTXT occurrence counts. The installed report is
`local/landscape-field-shapes-14c.jsonl` (266,212,079 bytes, SHA-256
`d1df5532c2916932e0fc21ec14f92002d3dc7760a3aa5162674b29cc668d37bf`). The
machine-readable summary is [preparation-14.json](preparation-14.json); the
source manifest is [source-manifest-14.json](source-manifest-14.json). See the
[landscape evidence contract](../docs/landscape-links.md). No terrain decoder
or renderer acceptance is claimed.
