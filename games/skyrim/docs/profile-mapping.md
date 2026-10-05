# Explicit Skyrim plugin profile mapping

`map-profile` builds Skyrim runtime slot candidates from a caller-supplied order.
It scans only the named plugins, reuses the shared TES4 reader, and fingerprints
the order input and every inspected plugin. It checks duplicate names, TES4 header
versions, direct masters, master-before-dependent order, slot budgets, and
header-flag light-plugin local-ID ranges.

The input is UTF-8 JSON with this shape:

```json
{
  "schema_version": 1,
  "active_plugins": [
    "Skyrim.esm",
    "Update.esm",
    "Dawnguard.esm"
  ]
}
```

`active_plugins` must list every active plugin in the order the caller intends to
model. Installed files, `Skyrim.ccc`, Creation-like names and master lists are not
used to infer activation or fill gaps. This first adapter intentionally accepts a
small normalized JSON list rather than guessing how a particular profile manager
interprets its files. No profile files were found in the Windows Known Folder
game directory, LocalAppData game directory, or expected Vortex Skyrim profile
directory, so no retail active order was available to test. This does not rule
out profiles held in another mod manager or another user-selected location.

```powershell
.\target\debug\skyrim-prep.exe map-profile `
  --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' `
  --order 'local\active-order.json' `
  --output 'local\profile-map.json'
```

Full plugins and header-flagged light plugins receive separate sequential slots
in the supplied order. A source identity can be encoded to a candidate runtime
FormID with `RuntimeProfile::runtime_form_id`, or an already runtime-encoded ID
can be resolved with `source_from_runtime_form_id`. Light IDs are checked against
the plugin HEDR range: 1.70 retains the `0x800..=0xFFF` range, while 1.71 accepts
the expanded `0x000..=0xFFF` range. The `.esl` suffix is reported independently
and does not decide slot class.

The output's `mapping.status` explicitly says that live game order and winning
overrides are not certified. Slot resolution does not prove that a FormID names an
existing record, that the executable consumed the supplied order, or that runtime
behavior matches the source graph. Cross-file FE values stored inside plugin
files remain outside this API until their on-disk context is validated. The map
does not decide winners or modify plugin bytes.
