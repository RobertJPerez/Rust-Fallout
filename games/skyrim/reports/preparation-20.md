# Preparation checkpoint 20: CELL context and water environment maps

Date: 2026-10-04

This checkpoint extends the existing Skyrim landscape trace with the Skyrim
CELL `XCCM` → `REGN` and `XLCN` → `LCTN` links, then adds asset evidence for
CELL `XWEM`. The pinned CELL schema identifies XWEM as a string; the installed
source values use NUL-terminated, `Data`-rooted paths. The adapter keeps each
field's exact byte length and SHA-256, a bounded raw value, terminator offset,
and malformed trailing bytes. It strips only the `Data` prefix, then reuses the
shared `AssetPath` and loose/BSA name lookup. It does not read texture payloads,
decompress archive members, choose winners, or infer water behavior.

The ten-plugin profile contains 77,337 CELL records and 841 LCTN records. It
has 85,754 selected CELL context FormID fields; 9,354 non-null resolvable links
have scanned target candidates, and none are left without a candidate or an
active-profile-dependent finding. This includes 338 `XCCM` links to REGN and
2,891 `XLCN` links to LCTN.

The scan found 109 XWEM fields across six plugins. Every field has a valid
terminator and Data root, yielding 16 unique normalized paths. The 23 BSA
indexes completed without failures; 106 references match names in `Skyrim -
Textures3.bsa`, no reference matches a loose file, and three references in
Dawnguard have no candidate for `textures/cubemaps/lavacubemap_e.dds`:

| Plugin | XWEM fields |
|---|---:|
| ccBGSSSE001-Fish.esm | 10 |
| ccBGSSSE025-AdvDSGS.esm | 2 |
| Dawnguard.esm | 20 |
| Dragonborn.esm | 13 |
| Skyrim.esm | 56 |
| Update.esm | 8 |

The three unmatched rows are retained in the local JSONL trace with their
source FormIDs. They establish no candidate among this install's loose files
and named archive entries; they do not prove the reference invalid. The profile
uses Steam build 24914197 and runtime 1.7.104.0, with four Creation pairs
installed. This does not establish full paid Anniversary Edition coverage.

The older source gaps remain: 101 LAND layer references have no scanned LTEX
candidate, and 30 CELL XCLR entries have no scanned REGN candidate. These are
separate from the three XWEM asset-name gaps. Original game files remained
read-only, and no gameplay or runtime acceptance is claimed.

All 69 Rust tests pass. Formatting, Clippy with warnings denied, and the
one-job build pass. The installed trace is
`local/cell-context-links-20e.jsonl` (448,928,990 bytes, SHA-256
`71051B4981AD4F8DCC6C5649B6C3D38ACFEF9C92D718D890EF27FACDB50FC9BA`). Its
source manifest is `source-manifest-20.json`; the machine-readable summary is
`preparation-20.json`. See `docs/landscape-links.md` for the field evidence and
normalization contract.
