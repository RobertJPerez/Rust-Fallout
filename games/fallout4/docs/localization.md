# Fallout 4 archive localization tables

The Fallout 4 preparation workspace now inventories localized string tables in
the direct `Data` BA2 files. It reads the installation without changing it,
checks each relevant archive against the frozen retail census before and after
the scan, and writes extracted table bytes and detailed key indexes only under
ignored `local/`.

The audit found 987 tables in 28 archives: 329 `.strings`, 329 `.dlstrings`,
and 329 `.ilstrings` files, containing 1,893,083 indexed keys. A separate
Mutagen comparison checked every key's presence, index offset, value length,
and raw value-byte hash: all 987 tables and all 1,893,083 keys matched, with no
missing keys, offset mismatches, value mismatches, or comparison exceptions.
No localized text is decoded into reports. A second BA2 backend also compared
all 85,632 members in those 28 archives, covering 4,236,548,717 decoded bytes;
all payloads matched and the archive fingerprints remained stable.

The path suffix census groups those tables into 84 logical archive/table
families. Twenty-one families have 11 observed locale suffixes and 63 have 12;
the observed suffixes are `cn`, `de`, `en`, `es`, `esmx`, `fr`, `it`, `ja`, `pl`,
`ptbr`, `ru`, and `zhhant`. Four families have key-ID sets that differ from
their `_en` table. Those differences are retained as physical observations;
they do not establish missing translations or incorrect game behavior. The
suffixes are labels read from paths, not evidence of the active profile's
language.

Reproduce the retail census with the existing frozen source report:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --locked --jobs 1 --bin audit-strings
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 run --locked --jobs 1 --bin audit-strings -- 'G:\SteamLibrary\steamapps\common\Fallout 4' 'local\proof-fo4-002\census.json' 'local\strings-audit-NNN'
py -3.13 tools/summarize_localization.py local/strings-audit-NNN
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --manifest-path tools/ba2-oracle/Cargo.toml
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-localization-archives.ps1 -OutputDirectory 'local\strings-ba2-oracle-NNN'
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-strings-oracle.ps1
New-Item -ItemType Directory -Path 'local\strings-oracle-NNN'
dotnet 'local\strings-oracle-bin\strings-oracle.dll' 'local\strings-audit-NNN' 'local\strings-oracle-NNN'
```

Choose fresh numeric suffixes for both output directories. The Mutagen source is
pinned in `sources.lock.json`; the isolated C# verifier reads the Rust-extracted
table bytes and uses Mutagen's `StringsLookupOverlay` to validate key lookup,
offsets and returned value bytes. The BA2 member extraction itself remains the
Rust audit's responsibility. The comparison uses the exact source revision and
paths recorded in `reports/localization-checkpoint.json`.

`verify-localization-archives.ps1` runs the already isolated BA2 oracle with
`--all` for every archive whose frozen census contains localization tables. It
requires exact source fingerprints before and after each archive and records
all member comparisons under `local/strings-ba2-oracle-NNN`.

The scope is physical archive content. This does not inspect loose or VFS
overrides, select archive precedence, confirm plugin activation, identify the
user's active language, decode text with a game code page, judge translation
quality, resolve localized record fields, or reproduce the game's language
fallback behavior. Connect the table catalog to verified active profile and
plugin evidence before using it for runtime lookup. Keep any future lookup
implementation on shared asset/archive contracts rather than adding a second
general runtime archive system.
