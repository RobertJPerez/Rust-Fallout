# Fallout 4 physical plugin dependencies

This audit builds a directed graph from the master names recorded in the TES4
headers of the completed physical census. It preserves each plugin's declared
master-table order and carries the source file hash. Name matching is
ASCII-case-insensitive; duplicate candidates, absent physical names and encoded
unsupported names remain explicit. Cycles are reported as graph findings.

The graph covers only the frozen physical plugin set. A unique physical master
file is a dependency candidate, not proof the plugin is activated. Graph edges
do not define runtime load order, ESL slot assignment, override winners or
content availability. Header flags are preserved and reported using the pinned
Fallout 4 flag definitions in `sources.lock.json`.

Reproduce from the completed census without rereading the retail installation:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bin audit-plugin-deps
.\local\target\debug\audit-plugin-deps.exe local/proof-fo4-002 local/plugin-deps-new
```

The output contains `graph.json` and `complete.json`; output must be a new
ignored `local/` directory. In the frozen census, 16 physical plugins declare
15 master edges. All 15 resolve to one physical filename candidate; there are no
duplicate physical names or declared-master cycles. The evidence hashes are in
`reports/plugin-dependency-checkpoint.json`; detailed node names stay in
`local/plugin-deps-001`.
