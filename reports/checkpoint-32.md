# Implementation checkpoint 32: leveled-list sources and inventory planning

The immutable catalogue now retains item, creature and NPC leveled lists. A
bounded graph connects their exact authored entries with base inventories and
actor templates, keeping duplicate edges, terminal dependencies and cycles.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 247 passing tests and Clippy with warnings denied |
| Original lists | 4,313: 3,479 LVLI / 437 LVLC / 397 LVLN |
| Independent source comparison | All 40,925 fields / 21,169 entries agree |
| Structural graph | All 14,185 nodes / 56,193 edges agree |
| Edge states | 40,058 internal / 16,135 terminal / zero unresolved |
| Original cycles | Zero in both iterative Kosaraju and Tarjan |
| Schema-domain mismatches | Three base-item edges retained and reported |
| Authored fixtures | Short/full words, ownership, deleted winners, duplicate links, two cycles and root closure |
| Cache/reorder | Exact definitions, graph and closure preserved |
| Malformed sources | Fourteen cases rejected by both readers before reports |
| Depth test | 100,000-node chain/cycle uses no recursive traversal |
| Fresh regressions | Complete base inventory and foreign/native-save/schema/quest/operand/header checks |
| Installation | All 464 files / 9,907,238,722 bytes still match baseline |

The source snapshot covers 247 source/tooling files at implementation
revision `3265e7cb9bbb6353ad6092a2f4c2bdd6fb2f8385`. Its digest is
`1bf1ff05f50ee32683286732da0ec83bc7acdaab852b49b7dbeac58c0fa046ff`. The fresh proof is
`local/leveled-lists-32-verified`.

M1 remains unfinished. No random draws, original default insertion or live loot
initialization occurred. No gameplay scenario is accepted. Next: persistent item
instances, explicit mutable state and shared primitive query components.

See [leveled lists](../docs/leveled-lists.md),
[the scoped receipt](checkpoint-32-leveled-lists.json) and
[verification](checkpoint-32-verification.json).
