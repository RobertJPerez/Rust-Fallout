# Implementation checkpoint 28: canonical script state

A separate Rust runtime crate now owns persistent script-instance identities,
exact local storage, live-reference links and pending event context. Immutable
winning definitions remain in the content layer.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 211 passing tests and Clippy with warnings denied |
| Independent schemas | All 80,627 winning units agree across 39,981 retained records |
| Unique locals | 11,172: 1,077 float / 8,067 integer / 2,028 reference |
| Duplicate declarations | Eleven; first-match lookup preserved |
| Canonical state probe | 2,483 instances / 9,144 numeric values / 2,028 references / 2,160 events |
| Snapshot | 2,381,445 bytes; exact round trip and persistent identities |
| Handle safety | Removed, reused, foreign-world and pre-restore handles rejected |
| Content changes | Changed source bytes and changed nonscript winners reject restoration |
| Authored fixtures | Cold, warm and reordered schema/state bytes agree |
| Malformed source cases | Ten rejected by both readers without a report |
| Runtime tests | Eleven cover typing, atomic mutation, bit patterns, references, events and malformed restoration |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

The probe values and event contexts are explicit engineering inputs. No original
running event list, constructor default, bytecode execution or retail scheduling
is claimed. Unknown declarations remain uninitialized; the three original stale
SCHR counts remain documented findings.

The source snapshot covers 218 source/tooling files at implementation
revision `131ba173bc201ca8194ea65e398853df24692553`. Its digest is
`0c1defe8b1eafa7f8f4ebc982d7882b081bf59ae0b8a8296ba2b178349a17cf2`. The fresh proof is
`local/script-state-28-verified-r2`.

M1 remains unfinished and no gameplay scenario is accepted. Next: native
filesystem saves with integrity, interruption and previous-slot recovery tests,
then foreign live-variable context and observable execution slices.

See [script runtime state](../docs/script-runtime-state.md),
[the scoped receipt](checkpoint-28-script-state.json) and
[verification](checkpoint-28-verification.json).
