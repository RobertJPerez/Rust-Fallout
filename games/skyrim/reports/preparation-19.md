# Preparation checkpoint 19: CELL region references

Date: 2026-10-04

This checkpoint extends the existing landscape trace to physical REGN records. For every CELL XCLR field, the trace retains its exact length, SHA-256 and 32-byte preview. Complete four-byte FormID entries pass through the existing owner-aware resolver and report physical REGN candidate counts. It preserves one-to-three trailing bytes as a separate malformed field instead of dropping them. The source-level format cross-check and pinned revision are recorded in sources.lock.json; region record bodies and runtime region selection remain uninterpreted.

The ten-plugin profile contains 77,337 CELL records, 389 REGN records and 34,008 XCLR fields, carrying 83,516 region references. All observed XCLR payload lengths are multiples of four, from 4 through 52 bytes; there are no repeated XCLR fields per CELL and no malformed XCLR tails. 83,486 references (99.9641%) have at least one scanned physical REGN candidate.

30 references resolve to the same source key, Skyrim.esm local ID 0x00106633, but no REGN candidate for that key is present in the supplied plugin set. The trace retains every source CELL, record offset, XCLR field hash and entry offset in local/cell-region-unmatched-19b.json. This is only a candidate gap in the scanned files; it does not classify the target as invalid or deleted. No candidate winner is selected.

| Plugin | CELL records | REGN records | XCLR fields | Region references | Malformed XCLR |
|---|---:|---:|---:|---:|---:|
| _ResourcePack.esl | 0 | 0 | 0 | 0 | 0 |
| ccBGSSSE001-Fish.esm | 610 | 0 | 551 | 3,072 | 0 |
| ccBGSSSE025-AdvDSGS.esm | 21 | 0 | 16 | 89 | 0 |
| ccBGSSSE037-Curios.esl | 0 | 0 | 0 | 0 | 0 |
| ccQDRSSE001-SurvivalMode.esl | 1 | 0 | 0 | 0 | 0 |
| Dawnguard.esm | 11,334 | 50 | 10,645 | 13,640 | 0 |
| Dragonborn.esm | 45,237 | 20 | 12,031 | 15,194 | 0 |
| HearthFires.esm | 247 | 0 | 195 | 1,171 | 0 |
| Skyrim.esm | 17,568 | 317 | 8,441 | 37,346 | 0 |
| Update.esm | 2,319 | 2 | 2,129 | 13,004 | 0 |

The existing LAND texture gap remains separate: 101 links to Skyrim.esm LTEX local ID 0x00000844 have no scanned candidate (74 BTXT and 27 ATXT). The installed command exits with findings status 2 for those 101 links and the 30 XCLR references.

The installation uses Steam build 24914197 and executable version 1.7.104.0. The ten-plugin, 23-archive profile has four installed Creation pairs and does not establish complete paid Anniversary content coverage. All ten source plugin fingerprints match the current census, and original game data remained read-only.

All 68 Rust tests pass. Formatting, Clippy with warnings denied, and the one-job build pass. The installed trace is local/cell-region-links-19b.jsonl (418,630,194 bytes, SHA-256 1efe13fd5ed86fb446b0d950a7d64479fa521cac8eca583dbe9b197db2deac02). The machine-readable summary is preparation-19.json; its source manifest is source-manifest-19.json. See docs/landscape-links.md for the evidence contract. No runtime region behavior or gameplay acceptance is claimed.
