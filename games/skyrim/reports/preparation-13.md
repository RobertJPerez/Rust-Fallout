# Preparation checkpoint 13: LAND group ancestry

Date: 2026-10-04

This checkpoint extends `trace-landscape-links` with each selected LAND record's
raw ancestor `GRUP` frames from the pinned shared visitor. The output retains
the frame offset, size, numeric kind, four label bytes and trailing bytes. The
group stack is bounded by the shared reader's validated extents; the synthetic
test confirms nested frames attach to a LAND record and expire before the next
out-of-group record.

All 54,079 installed LAND rows carry a six-frame path. Every one has numeric
group kinds `(0, 1, 4, 5, 6, 9)` in that order. The trace retains labels as raw
bytes and does not interpret group kinds as worldspace, cell, block, or grid
coordinates. Five of the ten scanned plugins contain LAND rows: Dawnguard has
2,223; Dragonborn 34,515; HearthFires 34; Skyrim 15,564; and Update 1,743.
The other five have no LAND records in this supplied install set.

This adds the physical ancestry needed for a later Skyrim-specific cell and
worldspace adapter without assigning meanings to labels. The LAND-to-LTEX,
LTEX-to-TXST/MATT/GRAS links, their physical candidates, and their limitations
are unchanged from checkpoint 12. In particular, 101 layer references still
resolve to Skyrim.esm local ID `0x00000844` with no scanned LTEX record. The
command completes with exit status 2 and a trailer for this finding. No terrain
geometry, texture pixels, world coordinates, winner selection, or gameplay is
claimed.

The current install census and plugin hashes remain the same as checkpoint 12:
Steam build 24914197, `SkyrimSE.exe` 1.7.104.0, ten plugin files and 23
archives. The four installed Creation pairs do not constitute the complete
paid Anniversary Edition corpus. The census still reports eight existing
blocking findings and zero reader failures.

The local report is `local/landscape-groups-13a.jsonl` (266,190,845 bytes,
SHA-256 `b08743a3c43b3024fd1f22de6930938fbf7ba7e60057737d355e42a3d3f31e86`);
it contains raw source hashes and bounded field evidence, not distributed
retail assets. The earlier
texture-set report has the same ten plugin fingerprints and provides the next
edge from physical TXST identities to normalized texture paths.

All 68 Rust tests pass. Formatting passes, Clippy passes with warnings denied,
and the one-job build passes. See the
[landscape link contract](../docs/landscape-links.md). No gameplay or renderer
acceptance is claimed.
