# Preparation checkpoint 21: location parent links

Date: 2026-10-04

This checkpoint extends the Skyrim CELL context trace through the LCTN
`PNAM` parent-location field. The adapter resolves each four-byte FormID using
the existing source-key logic and reports every scanned physical target
candidate. It does not choose a plugin override, assemble a winning hierarchy,
or infer runtime containment, location ownership, quest conditions, or
encounter behavior. Other LCTN fields retain their exact lengths and hashes
with bounded raw previews. Known but undecoded fields stay distinct from
unrecognized tags.

The installed ten-plugin profile contains 841 physical LCTN records with 7,123
subrecords. It has 811 four-byte PNAM fields and 30 LCTN records without PNAM.
All 811 links resolve to a source key with at least one scanned LCTN candidate;
none require a runtime profile or lack a scanned candidate. Candidate
multiplicity remains important: 383 links have one candidate, 263 have two,
26 have three, 64 have four, and 75 have five. Thus 428 links have multiple
physical candidates, and the trace leaves all of them unresolved as override
choices. The remaining 6,312 LCTN fields are retained as known opaque evidence;
there are no unrecognized LCTN tags in this scanned corpus.

| Plugin | LCTN records | PNAM links | Links with candidates |
| --- | ---: | ---: | ---: |
| Skyrim.esm | 638 | 617 | 617 |
| Dawnguard.esm | 62 | 58 | 58 |
| Dragonborn.esm | 98 | 94 | 94 |
| HearthFires.esm | 22 | 21 | 21 |
| Update.esm | 11 | 11 | 11 |
| ccBGSSSE001-Fish.esm | 9 | 9 | 9 |
| ccBGSSSE025-AdvDSGS.esm | 1 | 1 | 1 |
| **Total** | **841** | **811** | **811** |

The full trace also contains the previous CELL/LAND evidence: 135,023 selected
records, 77,337 CELLs, 54,079 LANDs, 83,516 XCLR entries (83,486 with scanned
REGN candidates and 30 without), and 85,754 selected CELL context links (9,354
non-null resolvable links with scanned candidates). The 109 XWEM strings remain
well formed: 106 match names in indexed archives, none match loose files, and
three Dawnguard references to `textures/cubemaps/lavacubemap_e.dds` have no
candidate among the scanned loose files and archive names. All 23 archive
indexes completed. These source gaps do not establish that a reference is
invalid.

The measured installation is Steam build 24914197 with executable version
1.7.104.0 and four installed Creation pairs. That inventory does not establish
the full paid Anniversary Edition content set. The records and archive names
are profile evidence only; no rendering, runtime location, or gameplay
acceptance is claimed. Original game files remained read-only.

All 69 Rust tests pass. Formatting, Clippy with warnings denied, and the
one-job build pass. The installed trace is
`local/location-parent-links-21b.jsonl` (451,019,680 bytes, SHA-256
`448C08447E32087B6F44C26EB874E85F21F22E5EDE459A02DE77B02F821B8FB1`) and has
a complete trailer. The source manifest and machine-readable summary are
`reports/source-manifest-21.json` and `reports/preparation-21.json`.

The read-only handoff review found New Vegas main clean at
`5a89c33bee93ccf6f6ff0cb489881ff9953a6c5b`; accepted shared-data proof remains
checkpoint 45 at `9265ef63f714cdf01ff02028640a582be9639c7a`. The scoped v3
development receipt is not gameplay acceptance. Fallout 4 main was observed
at `69f12091aa8b1b22530a8e203bdfbafc71c72a71` with active uncommitted work;
its inspected Papyrus code remains structural/static and does not provide an
agreed shared dialect API or a Papyrus runtime. Neither external working tree
was modified, and no second PEX decoder or VM was added here.
