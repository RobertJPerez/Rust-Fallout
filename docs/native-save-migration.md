# Explicit native save migration

`native-migrate-v2` imports the project's schema 2 native containers into a new
schema 3 repository. It never modifies the input or adopts an existing folder.
Normal native loading remains strict; unsupported schemas are not silently
converted during play or previous-slot recovery.

```powershell
.\target\release\fallout.exe native-migrate-v2 `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --file local/older-repository/current.frsv `
  --new-repository local/migrated-repository `
  --output local/native-migration.json
```

The input is opened with a retained write-denying read handle. File length is
bounded before allocation. Container versions, tags, extents, chunk hashes and
whole-file integrity must pass. A strict legacy JSON shape rejects unknown and
duplicate fields. Its campaign, revision, tick, cohort and length must agree
with META. Source metadata and its byte digests describe the original file;
the output receipt separately identifies the new state and container.

Before creating any destination, restoration checks the exact installed source
cohort, script schemas, identities, persistent links and chronology. All prior
canonical fields retain their values. The migration adds an item allocator at
one and leaves inventory banks absent: old saves had no observed live inventory
state. Empty initialized banks would invent a query result and are not inserted.

The new repository retains campaign identity and revision. Its own publication
generation starts at one because it is a separate repository; the original
generation remains in the source receipt. A normal strict load checks the result
after publication. The caller can then evolve the migrated world through ordinary
validated host operations.

The Rust API `save::format::migrate_v2` performs byte-level migration and returns
the original metadata plus a current snapshot. Callers must still use
`World::restore` before publishing it. The CLI completes that validation and
publication workflow. No public API downgrades populated schema 3 state.

Six tests cover exact preservation, schema disagreement, duplicate/unknown JSON,
rehashed metadata contradictions, corruption, budgets, source/allocator validation
and publication without changing the legacy input. The checkpoint also authors
a schema 2 envelope from fresh script-only engineering state, checks every prior
field, performs a fresh-process restore and rejects malformed imports before
repository creation. Independent C++ compares both containers' metadata and
checksums. Its `--schema-2` option is explicit and checks container integrity only;
it does not interpret the state JSON.

This imports our native format. Bethesda `.fos`, NVSE cosaves and third-party DLL
state remain separate work. Complete player/actor/quest/world persistence and
Windows directory/power-loss durability are still unfinished. No original
gameplay or retail-save compatibility is accepted.
