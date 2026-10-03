# Implementation checkpoint 25: static quest scripts and foreign declarations

Winning quest SCRI attachments now resolve into versioned script definitions.
Foreign operands select the attached quest script's first authored declaration.
Sparse/duplicate locals remain intact; current-script locals never stand in for
foreign ones. Placed references retain an explicit live-event-list dependency.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 191 passing tests and Clippy with warnings denied |
| Winning quests / SCRI attachments | 640 / 465; every attachment row and handle agrees |
| Winning compiled units / operand uses | 14,476 / 158,890; all binding digests match the full source scan |
| Foreign operands | 25,549; every declaration/dependency row independently agrees |
| Static quest declarations | 20,542; authored declarations only, with no live values |
| Placed-reference contexts | 5,007; live event lists remain required |
| Original source findings | Same two short quest headers and three stale script counts retained |
| Independent source fixture | Eleven quests, ten attachment states and ten foreign declaration states |
| Malformed quest inputs | Eleven native cases rejected without a report |
| Fresh regressions | Complete loaded/header/cache and operand/table/native/expression/descriptor comparisons |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

Static SCRI association does not establish a running event list's current script,
local values, scheduling or native effects. xNVSE's placed-reference path uses
ExtraScript/event-list state; the implementation does not substitute a base script.
Attachment lookup also rejects stale handles and mixed winning quest sources.

The source snapshot covers 192 source/tooling files at implementation
revision `6a1665e8cac22fe1aa72c030eadab83f506378e7`. Its digest is
`20ff2700affbfa97b436a9ad1d63ff4edcd20c8e7a21d439558ea21de7f7899f`. The fresh local proof is
`local/quest-scripts-25-verified`.

M1 remains unfinished. Independent compressed extraction and the LAND checksum
exception are next, followed by typed conditions and the first event/runtime
state slice. Original effective load order, runtime defaults, timing, command
behavior and gameplay acceptance remain open. No gameplay scenario is accepted.

See [quest script attachments](../docs/quest-script-attachments.md),
[the scoped receipt](checkpoint-25-quest-scripts.json) and
[verification](checkpoint-25-verification.json).
