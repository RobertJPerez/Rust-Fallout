# Skyrim preparation checkpoint 01

Verified October 3, 2026 (America/New_York). This is a working Rust data-preparation
checkpoint, with zero accepted gameplay scenarios.

The installed Steam game reports **1.7.104.0**, build **24914197**. The corpus has
the base game, Update, Dawnguard, HearthFires, Dragonborn, four Creation-named
plugins and `_ResourcePack.esl`. Owning the Anniversary Upgrade was not inferred;
its complete paid content set was not available for validation.

| Measured scope | Result |
| --- | ---: |
| Plugins / BSA 105 archives | 10 / 23 |
| Physical plugin records, including 10 TES4 headers | 1,188,920 |
| Groups | 139,287 |
| Indexed archive entries | 172,961 |
| Archived PEX members decompressed and hashed | 14,302 |
| VMAD attachment prefixes decoded | 24,631 |
| Properties preserved by prefix decoding | 73,386 |
| VMAD prefix decoding failures | 0 |
| Record-specific VMAD tails retained but not decoded | 10,178 |
| Distinct script paths referenced by non-removed primary attachments | 12,499 |
| Referenced script paths absent from scanned files | 7 |
| Hashed executable/plugin/archive bytes | 16,073,271,805 |

The initial scan exposed 2,561 VMAD v4 prefixes in Skyrim.esm. The loader was
extended with source-backed v4 scalar handling and a version gate for v5 arrays.
Both object formats occur in this installation. The final scan has no plugin or
archive open/parse failures, no missing required masters, and no duplicate archive
paths. All plugin/archive hashes and the executable hash match the initial scan.

The remaining seven input findings are preserved rather than repaired:

- `scripts/dlc01soulcairnskullpuzzlescript.pex`
- `scripts/dlc1testphilatronach.pex`
- `scripts/dlc1testphilvortexscript.pex`
- `scripts/dlc1testphilvortextrigscript.pex`
- `scripts/dlc2benthiclurkerfxscript.pex`
- `scripts/draguremergescript.pex`
- `scripts/lokirsdraugrresurrection.pex`

These are source references, not evidence that seven playable quests are broken.
Activation, override winners, unused/test records and route reachability still
need analysis. The CLI returns 2 to preserve those findings in automated runs.

Validation completed:

- `tools/cargo.ps1 test --locked --jobs 1`: 17 tests pass, including integrated
  synthetic plugins, LZ4 BSA payloads, VMAD layouts, bad counts/truncation, legacy
  versus expanded light IDs, missing dependencies and report-write protection.
- Formatting and Clippy with warnings denied pass for the application and the
  independent metadata tool.
- `tools/inspect-install.ps1`: completed the full declared census. This reads
  every plugin body and every archive index; only PEX archive members are extracted.
- The separate esplugin oracle agrees on all 10 physical record counts, HEDR
  versions, master lists and light-plugin statuses. A deliberately altered record
  count is rejected. This is not a VMAD or compressed-payload oracle comparison.

Evidence remains local and contains metadata/hashes, not bundled game assets:

- `local/census-20261004T012805Z-589420db.json`
- `local/install-20261004T012805Z-589420db.json`
- `local/plugin-oracle-final.json`
- `local/oracle-altered-input.json` and `local/oracle-altered-result.json`
- Earlier limited scan: `local/census-20261004T012305Z-33f94f27.json`

Exact hashes and source files are recorded in [preparation-01.json](preparation-01.json)
and [source-manifest.json](source-manifest.json). Shared NV checkpoint 45 is consumed
as a pinned Cargo dependency; no live NV worktree was modified.

Next: decode the 10,178 Skyrim VMAD fragment/alias tails and feed attachments into
the shared Papyrus work. Full/light runtime ID resolution, active load order,
Skyrim behavior, animation/physics, rendering, UI, saves and complete AE content
validation remain open. The game was not launched for a behavioral comparison.
