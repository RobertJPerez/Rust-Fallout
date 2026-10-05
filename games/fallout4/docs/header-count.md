# Fallout 4 header and physical group count

`Fallout4.esm`'s stored HEDR declares **1,741,853** records/groups. The pinned
shared parser counts 1,549,277 records including TES4 and 112,381 physical GRUP
headers. Excluding TES4, that gives 1,661,657 physical records plus group
headers. The FO4-specific group pass rechecked the full 330,776,576-byte file,
found 728 24-byte empty group headers, and observed nesting depths 0 through 5.

An isolated reader at the pinned Mutagen revision independently enumerated
1,549,276 major records, matching the Rust walk's record count excluding TES4.
Its Fallout 4 `GetRecordCount()` calculation returns 1,661,201. Therefore the
Mutagen major-record walk agrees, while its model count and the physical
record-plus-GRUP count remain 80,652 and 80,196 below the stored HEDR value.
Mutagen's calculated extra group contribution is 111,925, 456 fewer than the
112,381 physical group headers. The 728 empty headers do not by themselves
explain that difference. The pinned source defines `GetRecordCount()` as the
major-record enumeration plus one for each populated typed top-level record
cache, then adds Fallout 4-specific CELL block/sub-block and nested-group terms,
worldspace cell block/sub-block and nested-group terms, and quest/dialogue
response terms. Its count is therefore a writer model over typed collections,
not a one-for-one count of physical GRUP headers. This audit did not split the
111,925 contribution among each formula term.

The Rust census does not stop or allocate based on HEDR, and neither audit edits
the plugin. Matching major-record totals makes a simple missing-record walk less
likely, but the independent reader is not the Bethesda engine or xEdit. The
exact retail HEDR rule and why this header differs remain unresolved. Do not
rewrite the source header or treat this comparison as runtime compatibility.

The raw group ledger and independent count report stay under ignored `local/`:
`local/group-count-001/` and `local/header-count-001/`. Their hashes and source
revisions are recorded in
[`reports/header-count-checkpoint.json`](../reports/header-count-checkpoint.json).

Reproduce with a fresh output directory for each tool:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --locked --jobs 1 --bin audit-groups
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 run --locked --jobs 1 --bin audit-groups -- `
  'G:\SteamLibrary\steamapps\common\Fallout 4' 'local/proof-fo4-002' 'local/group-count-NNN'
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-record-count-oracle.ps1
dotnet local/dotnet-artifacts/record-count-oracle/bin/record-count-oracle/release/fo4-record-count-oracle.dll `
  'G:\SteamLibrary\steamapps\common\Fallout 4' 'local/proof-fo4-002' 'local/header-count-NNN'
```

The C# oracle verifies the pinned Mutagen checkout and keeps its build artifacts
under ignored `local/`. It is an offline reference tool, not a Rust runtime
dependency.
