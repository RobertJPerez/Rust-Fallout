# Leveled-list sources and dependency planning

The immutable catalogue now retains winning `LVLI`, `LVLC` and `LVLN` records.
Every field has a source position, length and hash, and each decoded record keeps
its original bytes. Deleted winners remain tombstones. Item, owner and global
links resolve through the source plugin's master table and winning headers.

`LVLO` level and count words stay as exact 16-bit values. Level/count padding is
also retained. The pinned [common entry schema](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsCommon.pas#L9062-L9084)
makes count and trailing padding optional; the decoder preserves absence in 8-,
10- and 12-byte layouts without inserting the editor's default count. Odd partial
words and unsupported lengths fail. Optional following `COED` fields preserve
ownership unions and condition bits as described in [base inventory](base-inventory.md).
Duplicate controls and ambiguous extra associations remain source findings.

`LVLD` chance-none bytes, `LVLF` flags and item-list `LVLG` globals remain authored
inputs. Known bits receive schema labels; unknown bits remain in the raw byte.
The use-all label belongs to item lists only. The [pinned FNV definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas#L6647-L6700)
provide each list's item-kind domain. Source-domain checks are informational and
do not decide runtime eligibility, inheritance or selection.

The [pinned xNVSE runtime layout](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameForms.h#L3142-L3193)
uses signed 16-bit level/count fields and separate script-added entries. This
does not establish the original loader's conversion or selection rules. The
catalogue keeps raw disk bits and performs no random draws or initialization.
Source-lock scopes record the selected reads; no upstream code is imported.

The structural graph combines all base inventory entries, actor template links
and leveled-list entries. Its nodes include deleted inventory/list records.
Edges retain source keys, physical field positions, roles, target states and
schema-domain results. Duplicate edges remain distinct. Ownership and chance
globals stay separate source bindings rather than expansion links.

Graph construction requires matching full source cohorts and winning-header
digests. Node/edge budgets are checked before insertion. Rust uses iterative
Kosaraju to identify strongly connected components; offline C++ independently
uses iterative Tarjan. Both avoid recursive call-stack growth. Cycles remain
reported facts: they are not dropped or converted into runtime expansion rules.
Even template links with unset inventory-inheritance flags remain authored edges.

An optional canonical root produces a bounded structural closure within this
domain. It lists reachable inventory/list/template nodes and every outgoing edge,
including terminal item dependencies and unresolved targets. Missing or deleted
roots fail. Terminal item bodies and full asset dependencies are outside this
plan. All defined graph targets are followed for inspection, including schema
mismatches; this is not a runtime admission policy.

The original inspection order contains 4,313 lists: 3,479 item, 437 creature and
397 NPC lists. Rust and C++ agree on all 40,925 fields and 21,169 entries. Their
combined graph has 14,185 nodes and 56,193 edges: 40,058 internal links and 16,135
terminal links. No targets are unresolved, and neither algorithm finds a cycle.
Three base-item links fall outside the selected xEdit item-kind domain; their
exact identities and field positions remain in the scoped receipt.

```powershell
.\target\release\fallout.exe leveled-lists --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --output local/leveled-new.json
.\target\release\fallout-evidence.exe --checkpoint 32 --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --run-directory local/leveled-repeat --no-publish
```

For closure, add `--root-plugin` and decimal `--root-id`, using the canonical
origin plugin and local ID, never a transient runtime load-order high byte.
The inspection order remains an explicit test input rather than verified retail
load order. Findings are retained in a report with exit code 1; malformed inputs
fail before any report is published.

Seven source/planning tests and two graph tests cover exact bits, short layouts,
namespaces, ownership, deleted targets, duplicate links, cohorts, closure budgets
and a 100,000-node chain/cycle. The checkpoint compares every original field and
graph, authored cold/warm/reordered cyclic fixtures, and fourteen malformed cases.
Fresh base-inventory and its runtime/save/quest/header regressions cover shared
source classification. No original assets appear in published receipts.

Original probability rules, random draw order, level/count coercion, template
inheritance, script-added entries, respawn and mutable item persistence remain
unimplemented. No gameplay scenario is accepted by this source checkpoint.
