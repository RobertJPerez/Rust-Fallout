# Source-bound form lists

The `fallout-data::form_lists` catalogue reads winning `FLST` records through the
shared immutable plugin index. Each repeated `LNAM` is one exact four-byte form
ID. Physical field order, repeated members, nulls, unknown fields and raw record
bytes are retained. There is no editor sorting, list flattening or duplicate
removal. An explicitly empty decoded list remains different from a tombstone.

Each member resolves in its defining source's master namespace. Bindings retain
the raw word, canonical key, missing/deleted/defined state and the winning target's
kind, flags, physical offset and plugin. Whole-record overrides replace older list
bodies. A winning deletion never inherits the old members. Source receipts and the
winning-header digest keep the catalogue tied to the complete indexed cohort.

The dependency graph reports one edge per physical member, including duplicates
and unresolved targets. It follows defined lists for structural inspection and
retains other forms as terminals. The shared iterative Kosaraju implementation
finds cycles without a recursive native call stack; the independent C++ reader
uses iterative Tarjan. Breadth-first closure visits each reachable list once and
keeps every selected edge. Canonical ordering makes reports reproducible without
changing authored member order. The graph exposes read-only slices and budgets
actual immutable members rather than mutable display counters.

Limits cover records, decoded bytes, fields, members, graph nodes/edges and closure
nodes/edges. Remaining decoded-byte capacity tightens the record read before
allocation. Truncated or malformed fields and corrupt compressed inputs fail;
diagnostic checksum data cannot supply this catalogue. The catalogue keeps owned
record bodies after the store closes.

```powershell
.\target\release\fallout.exe form-lists `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --output local/form-lists-new.json
```

Optional `--index-cache` uses the existing source-bound cache. A structural root
can be selected with `--root-plugin` and a nonzero canonical `--root-id`; missing
or deleted roots fail. This selects dependencies, not an inventory count policy.

The installed-data development comparison matches all 608 winning lists, 6,698
fields and 6,090 member occurrences across Rust and a direct original-source C++
reader. It finds two nested-list edges and no cycles or unresolved members. The
final checkpoint receipt records the committed proof, fixture/cache comparisons
and the fresh shared-query/source/runtime regression chain separately.

Eight Rust tests cover physical order, duplicates, retained opaque bytes,
source namespaces, whole overrides, tombstones, null/missing/deleted bindings,
cycles, closure, malformed/extended fields, corruption and capacity limits.
The shared traversal also retains its 100,000-node chain/cycle test. Independent
fixture checks cover cold/warm reuse and changed-source invalidation. Ten malformed
field cases must fail in both source readers without publishing a success report.

The pinned [xEdit FLST definition and editor sort helper](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
and [xNVSE BGSListForm declaration](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameForms.h)
are selected source references, recorded with ranges and full-file hashes in
`sources.lock.json`. No upstream implementation is copied or linked.

The runtime still rejects `GetItemCount` list expansion as unverified. Authored
members do not establish live script-added members, list lifetime/persistence,
duplicate/null/nested-list handling or original argument and numeric return
coercion. VM execution and measured original-game behavior remain open. This
checkpoint accepts no gameplay scenario.
