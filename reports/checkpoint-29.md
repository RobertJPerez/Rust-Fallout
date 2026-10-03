# Implementation checkpoint 29: native script-state saves

Native repositories now publish owned canonical captures on a worker, retain a
verified previous slot and restore source-bound state in a fresh process. Campaign
identities and checked revisions reject stale or conflicting captures.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 222 passing tests and Clippy with warnings denied |
| Native state | 2,483 instances / 2,160 pending events; exact worker and cold-process round trips |
| Later snapshot | 2,381,546 bytes; exact numeric bits, identities and event context |
| Container reader | Independent C++ extents, metadata and SHA-256 agree for both slots |
| Malformed containers | 27 rejected by both readers without publishing metadata |
| Interruption | Real child killed at five write/sync/publication stages; complete old/new slots and released lock |
| Recovery | Strict corruption rejection, explicit previous load and explicit repair |
| Conflicts | Other campaigns, older revisions and different same-revision branches rejected |
| Migration | Explicit schema 1 to 2 migration preserves state and requires campaign identity |
| Schema regression | All 80,627 winning compiled schemas agree; cold/warm/reordered fixtures and ten malformed source cases repeated |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

The snapshot SHA-256 is
`f0655cb269a06daf8106fe408c7b4d0be167b0904e9c0952cc56bee6df556ed0`.
The values are explicit engineering inputs. Original live state, initialization,
execution and scheduling remain unmeasured. The independent C++ reader does not
interpret canonical JSON semantics.

The source snapshot covers 226 source/tooling files at implementation
revision `a8db01da0ae082f38893454d39620795241958e3`. Its digest is
`a6e4ec5931024f8165a0e41fe68d6054e0d97ed5f85c0b5bab482deaa9fa3b16`. The fresh proof is
`local/native-saves-29-verified`.

Windows NTFS process interruption was tested. Windows directory/power-loss
durability was not verified. Full mutable world components, Bethesda `.fos`
compatibility, NVSE cosaves and third-party DLL state remain unfinished.

M1 remains unfinished and no gameplay scenario is accepted. Next: foreign local
access through explicit live quest/placed event lists and observable execution.

See [native saves](../docs/native-saves.md),
[the scoped receipt](checkpoint-29-native-saves.json) and
[verification](checkpoint-29-verification.json).
