# Skyrim texture-set asset evidence

`trace-texture-sets` scans physical `TXST` records and exports their `TX00`-
`TX07` texture fields. The adapter uses the pinned TES5 record definitions for
field names and meanings and the pinned Mutagen schema's `SkyrimTexture` asset
links as a second layout reference. `TX00` is diffuse, `TX01` normal/gloss,
`TX02` environment-mask/subsurface tint, `TX03` glow/detail, `TX04` height,
`TX05` environment, `TX06` multilayer, and `TX07` backlight-mask/specular. The
schema references are
[TES5Edit's pinned TXST definition](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsTES5.pas#L5058-L5077)
and [Mutagen's pinned texture-set schema](https://github.com/Mutagen-Modding/Mutagen/blob/0188012c607ce8bb283d2704400d37737f089134/Mutagen.Bethesda.Skyrim/Records/Major%20Records/TextureSet.xml#L243-L265).

Each `texture-set` row keeps the physical plugin filename, FormID, record offset
and flags. Texture fields retain their raw path bytes, bounded decoded path,
normalization status, subrecord offset, hash, terminator evidence and duplicate
slot status. For lookup, the adapter maps a normalized `SkyrimTexture` field to
one Data-root candidate by prepending `textures/` exactly once. The output keeps
the normalized field path and the resolved `lookup.data_asset_path` distinct.
It checks that exact path under the loose `Data` tree and in each immediate BSA
105 archive index, using the shared `AssetPath` normalizer and archive backend.

The matcher does not decompress texture members, decode DDS pixels, interpret
material or decal behavior, select an archive winner, resolve plugin overrides
or establish runtime use. `OBND`, `DODT`, `DNAM` and unknown subrecords are
preserved as bounded raw bytes and full-field hashes; their semantics are not
decoded here. Missing terminators, invalid paths, duplicate slots, archive
index failures, hash-only entries, invalid archive paths and unmatched
references remain explicit findings.

Collection is bounded to 100,000 `TXST` records and 100,000 visited subrecords
across all supplied plugins. Raw path and auxiliary-field evidence is capped at
1,024 bytes per field while hashes cover the full field contents.

On Steam build 24914197 / runtime 1.7.104.0, the read-only scan covered the ten
installed plugin files and 23 BSA indexes. It found 790 `TXST` records, 2,182
texture fields and 1,149 unique resolved paths. Archive names matched 2,153
field references; 29 references had no loose or indexed archive candidate.
There were no malformed paths, duplicate slots, unknown subrecords or archive
index failures. The 29 unmatched fields are listed in the ignored local JSONL
evidence. This result covers the installed SE profile and four included
Creation plugin/archive pairs; it does not establish full paid Anniversary
content coverage or prove the unmatched assets are invalid.

Run the trace with one or more explicit plugin paths:

```powershell
.\target\debug\skyrim-prep.exe trace-texture-sets --data 'G:\Games\Skyrim Special Edition\Data' --plugin 'G:\Games\Skyrim Special Edition\Data\Skyrim.esm' --plugin 'G:\Games\Skyrim Special Edition\Data\Update.esm' --output 'local\texture-sets.jsonl'
```

See [`preparation-11.md`](../reports/preparation-11.md) for the measured
checkpoint and validation details.
