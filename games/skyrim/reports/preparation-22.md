# Preparation checkpoint 22: player movement source profile

Date: 2026-10-04

This checkpoint follows the Skyrim player movement records without building a
second plugin reader. It traces the exact NUL-terminated `Player` `NPC_ EDID`
through `RNAM` race links, then follows each race's six movement defaults and
repeated `MTYP` entries to physical `MOVT` records. It retains source hashes,
physical candidates, exact link bytes, float bit patterns, and bounded opaque
evidence. It does not select winning overrides or infer movement behavior.

The installed ten-plugin profile contains 6,626 scanned NPC rows, one explicit
Player record, 214 physical races, and 107 physical movement records. The
Player is `Skyrim.esm` FormID `0x00000007`; its `RNAM` points to source key
`skyrim.esm:0x00013746` with two scanned physical candidates. Both candidates
are labeled `NordRace`, one in `Skyrim.esm` and one in `Update.esm`. They remain
unselected. Neither row carries any of the movement links or race `SPED`
override represented by this adapter, so the trace does not substitute an
assumed race default.

Across all scanned race rows, 239 encoded movement links cover the six named
defaults and `MTYP`. Every link has a scanned target candidate; all 239
race-to-movement links have one candidate each. The Player's `RNAM` link is the
one link with multiple candidates. No link lacks a candidate, requires an
active profile, or is malformed. The scan retains 107 `MOVT SPED` fields and
11 `RACE SPED` fields, with no malformed float payloads. The 107 movement
records comprise 15 records below version 28 and 92 at version 28 or later,
so both ten-slot and eleven-slot movement speed layouts occur in this corpus.
All movement values remain source data: the exporter performs no unit or
angle conversion, speed formula, race selection, or gameplay claim.

The measured installation remains Steam build 24914197 with executable
version 1.7.104.0 and the same four installed Creation plugins recorded in
checkpoint 21. This verifies the source adapter against the installed updated
Special Edition profile and both observed `MOVT SPED` layouts. It does not
verify the complete paid Anniversary Edition content set; Creation file
presence alone is not treated as an AE ownership check. Retail files remained
read-only.

All 74 Rust tests pass. Formatting, Clippy with warnings denied, and the
one-job build pass. The final installed trace is
`local/movement-profile-22b.jsonl` (46,630,517 bytes, SHA-256
`14A0BEE11E0A1D3B3C29562F2681D231B5A3CDD343F79E9B47058F326E86AA0B`) with a
complete trailer. See the machine-readable evidence in
`reports/preparation-22.json` and the source inventory in
`reports/source-manifest-22.json`.

The record layouts used here are pinned in the existing TES5Edit and Mutagen
schema references. Their definitions identify `NPC_ RNAM` as a race link,
the six race movement defaults and `MTYP` as movement links, eleven `RACE`
speed slots, and the version-28 `MOVT SPED` change with three `INAM`
thresholds. See the [TES5Edit definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsTES5.pas#L7223),
[Mutagen NPC schema](https://raw.githubusercontent.com/Mutagen-Modding/Mutagen/0188012c607ce8bb283d2704400d37737f089134/Mutagen.Bethesda.Skyrim/Records/Major%20Records/Npc.xml),
[Mutagen race schema](https://raw.githubusercontent.com/Mutagen-Modding/Mutagen/0188012c607ce8bb283d2704400d37737f089134/Mutagen.Bethesda.Skyrim/Records/Major%20Records/Race.xml), and
[Mutagen movement-type schema](https://raw.githubusercontent.com/Mutagen-Modding/Mutagen/0188012c607ce8bb283d2704400d37737f089134/Mutagen.Bethesda.Skyrim/Records/Major%20Records/MovementType.xml).
