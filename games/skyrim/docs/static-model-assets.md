# Skyrim static model asset evidence

`export-stat-models` streams physical `STAT` records from one plugin. It uses
the pinned shared TES4 visitor to skip unrelated record bodies and keeps each
record's source FormID and file offset. Field offsets are relative to the decoded
record payload, including when that record was compressed.

This command and `trace-stat-assets` cover `STAT` only. The separate
`export-generic-models` command covers the pinned schema's other generic-model
record kinds; see the [generic-model evidence contract](generic-model-assets.md).

The Skyrim schema adapter retains `EDID` bytes/text, records each `MODL` path,
and hashes each `MODT` and `MODS` payload without interpreting their binary
contents. It also separates the four fixed 260-byte names inside `MNAM` into LOD
levels 0 through 3. The pinned record definitions specify the `STAT` generic
model attachment and those four LOD fields; this implementation remains a
source-data adapter, not a renderer.
See the [pinned record definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsTES5.pas#L9751)
and [generic model field layout](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsTES5.pas#L847).

`trace-stat-assets` accepts one or more repeated `--plugin` inputs and checks
each valid source path against loose files under the selected `Data` directory
and every BSA 105 names-only index in that directory. It indexes the archive set
once for all supplied source plugins.
For model paths not already rooted at `meshes/`, it reports both the exact path
and a separate `meshes/`-rooted candidate. It hashes every inspected archive,
retains every matching archive/member name, and does not decompress BSA members,
choose duplicate paths, apply archive invalidation rules, resolve plugin
overrides, or claim runtime loading. Archive failures, hash-only entries,
invalid names and source fields with malformed paths remain visible in the
completion record and exit status.

On the installed Skyrim SE build 24914197 / runtime 1.7.104.0, the all-plugin
pass covered ten installed plugins and found 12,626 `STAT` records, 12,618
`MODL` fields, 1,096 `MNAM` fields and 3,085 LOD path strings. Eight records
have no `MODL`; all observed model and LOD strings passed bounded path checks.
The 23 installed BSA indexes contained a candidate for 15,659 of 15,703 source
path references; 44 had no loose-file or archive match in this installation.
All 15,659 matches came from the separate `meshes/`-rooted candidate; none came
from the exact field-path candidate.
The 44 are distributed across `Dawnguard.esm` (6), `Dragonborn.esm` (1),
`Skyrim.esm` (33) and `Update.esm` (4). Four absent `Update.esm` candidates
name paths under `CreationClub\\_Shared\\Dungeons\\AyleidRuins`; this is only
evidence that those paths are not supplied by the currently installed loose
files or indexed archives. It does not establish ownership or a complete paid
Anniversary corpus. Thirty source references have members in multiple archives;
all matching members remain listed without a winner decision.
The unmatched source FormIDs, editor IDs, fields and paths are listed in ignored
local evidence at `local/stat-asset-gaps-07c.tsv`; source references spanning
multiple archives are listed at `local/stat-asset-overlaps-07c.tsv`. The full
per-record output for each scanned plugin is `local/stat-models-07c-<plugin>.jsonl`;
it preserves source offsets and hashes opaque `MODT`/`MODS` fields without
decoding them.

One positive example is source path
`LoadScreenArt\\LoadScreenMRaltar01.nif`, indexed in
`Skyrim - Meshes1.bsa` as
`meshes/LoadScreenArt/LoadScreenMRaltar01.nif`. Its header probe reports the
Skyrim SE stream tuple. This confirms one source-to-index mapping and header
format only; it does not validate NIF blocks or game behavior. The paid
Anniversary corpus remains incomplete.

Run one or more plugins through the trace with repeated `--plugin` options:

```powershell
.\target\debug\skyrim-prep.exe export-stat-models --plugin 'G:\\Games\\Skyrim Special Edition\\Data\\Skyrim.esm' --output 'local\\stat-models.jsonl'
.\target\debug\skyrim-prep.exe trace-stat-assets --data 'G:\\Games\\Skyrim Special Edition\\Data' --plugin 'G:\\Games\\Skyrim Special Edition\\Data\\Skyrim.esm' --plugin 'G:\\Games\\Skyrim Special Edition\\Data\\Update.esm' --output 'local\\stat-assets.jsonl'
```

The reports are read-only observations. Neither command certifies a complete
asset inventory or a playable Skyrim profile.
