# Skyrim generic-model references

`export-generic-models` extracts `MODL` paths from 39 non-`STAT` record kinds
whose pinned Skyrim schema declares the generic model structure. This extends
the existing `STAT` export without changing its specialized `MNAM` LOD handling.
The schema's generic model is a model filename plus opaque metadata members;
the extractor retains source ownership, file-local FormID, record offset,
decoded-record field offset, field hash, raw path bytes, editor-ID bytes and the
normalized path. Malformed paths stay visible as findings.

The allowlist is `ACTI`, `ADDN`, `ALCH`, `AMMO`, `ANIO`, `APPA`, `ARTO`, `BOOK`,
`BPTD`, `CAMS`, `CLMT`, `CONT`, `DOOR`, `EXPL`, `FLOR`, `FURN`, `GRAS`, `HAZD`,
`HDPT`, `IDLM`, `INGR`, `IPCT`, `KEYM`, `LIGH`, `LVLN`, `MATO`, `MISC`, `MSTT`,
`PROJ`, `QUST`, `RACE`, `SCOL`, `SCRL`, `TACT`, `TREE`, `WEAP`, `WRLD` and
`WTHR`. `STAT` remains in the dedicated static-model adapter. The pinned schema
defines the shared filename member and demonstrates its use in `ACTI`, `DOOR`
and `STAT`; this supports the explicit allowlist and does not turn other
same-named fields into paths. See the [generic model definition](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsTES5.pas#L847), [activator definition](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsTES5.pas#L3057), and [static definition](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsTES5.pas#L9751).

The adapter uses the pinned shared plugin visitor, source digest and `AssetPath`
normalizer. It does not decode `MODT`/`MODS`, resolve overrides or active load
order, enumerate placed references, choose archive winners, or claim runtime
use. It exports source references only; `trace-stat-assets` remains explicitly
STAT-only. Compare candidate paths against the archive index as a separate step.

On the measured Steam build 24914197 / executable 1.7.104.0, the ten installed
plugin files yielded 19,636 schema-selected records and 15,079 `MODL` fields;
all 15,079 normalized without path findings. This is a source-field census of
the installed files, not full paid Anniversary Edition content coverage.

The all-plugin scan found the stream-83 Ayleid pressure plate by its
`Update.esm` `ACTI` model field. The source record is file-local `0x01003206`,
editor ID `ccBGS_ARTrigPressurePlate01`, at source offset 808,329; its stored path
is `CreationClub\\_Shared\\Dungeons\\AyleidRuins\\Interior\\Triggers\\ARTrigPressurePlate01.NIF`.
The matching member is present in `Skyrim - Meshes0.bsa` and its 20,755-byte
payload has the legacy stream-83 tuple recorded in the checkpoint-08 header
evidence. A targeted scan found no generic-model field references to the other
four unmatched `Update.esm` Ayleid `STAT` paths, and no exact match among
installed `STAT` `MODL` fields for the pressure plate. These results connect a
source record to an archive member; they do not establish activation, archive
precedence, or whether this installed runtime consumes the legacy NIF.

The ignored local JSONL exports are `local/generic-models-09a-<plugin>.jsonl`.
Each report includes source fingerprints and a completion trailer; incomplete
reads do not produce a successful trailer. Synthetic fixtures cover an
allowlisted `ACTI`, exclusion of `CELL` and the separately handled `STAT`, path
normalization and visible unterminated-path findings.

```powershell
.\target\debug\skyrim-prep.exe export-generic-models --plugin 'G:\\SteamLibrary\\steamapps\\common\\Skyrim Special Edition\\Data\\Update.esm' --output 'local\\generic-models.jsonl'
```

Machine-readable summary: [`preparation-09.json`](../reports/preparation-09.json).
