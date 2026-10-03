# Implementation checkpoint 34: explicit native save migration

Schema 2 native saves now import into new source-bound schema 3 repositories.
Prior state is preserved exactly; unknown inventory contents stay uninitialized.
Strict ordinary loading still rejects unsupported schemas.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 267 passing tests and Clippy with warnings denied |
| Migration tests | Six cover preservation, metadata/JSON disagreement, bounds and source validation |
| Full prior state | Every script/reference/event/clock/identity field matches |
| Input protection | Read-only handle retained; old file hash unchanged |
| Invalid imports | Eight rejected before any repository or output creation |
| Cold restore | Full canonical digest and new container metadata agree |
| Independent reader | Both schema 2 and schema 3 metadata/extents/checksums agree |
| Fresh regressions | Complete item/leveled/inventory/foreign/save/schema/quest/operand/header checks |
| Binary binding | Every dependent helper has start/end hash guards |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The migrated snapshot has 2,381,581 bytes and SHA-256
`79d52ad95a7020f65048f152ccb4550314f7873c5e2425b30997d8a6d5236419`. Its values are disclosed engineering inputs.
The proof fixture is authored from fresh script-only source-bound state; no
populated inventory state is discarded. Independent checks cover container
integrity rather than JSON semantics or original-save compatibility.

The source snapshot covers 254 files at implementation revision
`5fc8d685a4d1a834e29e4003d6e5bca364f02219`, digest `85136ccebe978d54dcee897627a968abc5dd1e956110cd9ff2041bb971b50d30`. Fresh evidence is under
`local/native-migration-34-verified`.

M1 and all gameplay acceptance remain unfinished. Next: source-bound item
validation and shared primitive query adapters.

See [native migration](../docs/native-save-migration.md),
[the scoped receipt](checkpoint-34-native-migration.json) and
[verification](checkpoint-34-verification.json).
