# Implementation checkpoint 22: quest and dialogue ownership

Rust now retains original QUST/INFO/DIAL sections and assigns conditions and
embedded scripts to their authored owners. Repeated stage numbers stay distinct;
missing parent links and short headers remain diagnostic source findings.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 176 passing tests and Clippy with warnings denied |
| Quest / topic / INFO records | 642 / 21,560 / 28,933 |
| Original fields / sections | 593,244 / 176,792; every typed/owner digest agrees independently |
| Script units / compiled bodies | 59,225 / 9,709; metadata also matches the prior table proof |
| Conditions | 71,416; INFO, general quest, log-entry and target owners remain separate |
| Source findings | Eighteen orphaned shared-info entries and two short quest headers |
| Negative coverage | Twelve malformed bundles rejected; optional-layout/orphan fixture agrees |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

The complete diagnostic CLI report returns 1 for the original findings. No quest
parent, missing header word or editor default is invented. Strict unrelated LAND
checksum enforcement remains intact.

The source snapshot covers 169 source/tooling files at implementation
revision `39f6a224c87c0c7fd8ccb388a9b539cbfa28ba3d`. Its digest is
`bcebdb09e3e75bfe876655d67f7957ac64c90dd6e6cd814efadb33b4b2d0d0a7`. The fresh local proof is
`local/narrative-22-verified2`.

M1 remains unfinished. Canonical winning script identity, INFO parent membership,
loaded subjects, condition queries, quest state and dialogue behavior remain open.
Prior rendering evidence keeps its own scope; no gameplay scenario is accepted.

See [narrative structure](../docs/narrative-structure.md),
[the scoped receipt](checkpoint-22-narrative-structure.json) and
[verification](checkpoint-22-verification.json).
