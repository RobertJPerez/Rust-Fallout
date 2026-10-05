# Placed model source references

`trace-placed-uses` connects one source `ACTI` record to placed `REFR` records
whose four-byte `NAME` field identifies that base object. It takes an exact
plugin filename and file-local FormID, then scans the target plugin and any
explicitly supplied source plugins. Each physical record identity is resolved
through that file's declared master list and retains its origin plugin and local
ID. Same-numbered FormIDs from different plugins therefore do not collide.

The command reuses `fallout-data`'s selected-record visitor, record/subrecord
framing, source digest and Skyrim asset-path normalizer. It preserves source
offsets, record flags, cell/world group context, raw `NAME` bytes, target
`EDID`/`MODL` fields, per-file hashes and counts. Malformed `NAME` fields and
unresolved master indices remain findings. Light-plugin records retain their
file-local identity from the plugin header, while cross-file `FE` links stay
unresolved without a caller-provided active light-slot map.

Example for the currently measured install:

```powershell
.\target\debug\skyrim-prep.exe trace-placed-uses `
  --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' `
  --target-plugin Update.esm --form-id 0x01003206 `
  --plugin Skyrim.esm --plugin Update.esm --plugin Dawnguard.esm `
  --plugin HearthFires.esm --plugin Dragonborn.esm `
  --output 'local\placed-uses.json'
```

This is a source-reference census, not an active-profile mapper: it does not
infer runtime plugin order, winning overrides, archive winners, activation,
initial enablement or whether a reference is reached in play. It covers `REFR`
only, not temporary references, actors, leveled spawns, or Papyrus-created
objects. A zero-match result means only that no matching `REFR NAME` occurred in
the explicitly scanned sources. The current installed SE profile does not
represent the complete paid Anniversary Edition content corpus. The same
identity handling accepts validated Skyrim light-plugin record IDs, including
a synthetic HEDR 1.71 expanded-ID case, but this command's measured scan does
not claim complete AE compatibility.

The first measured use is checkpoint 10: the Ayleid pressure-plate `ACTI` in
`Update.esm` has no matching placed `REFR` in the ten installed plugin files.
The exact report is retained under ignored `local/` evidence and summarized in
[`preparation-10.md`](../reports/preparation-10.md).
