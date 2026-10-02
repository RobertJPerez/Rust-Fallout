# Implementation checkpoint 03: scene graphs and geometry

The Rust content pipeline now decodes actual NV scene graphs, local/composed
transforms, triangle lists/strips, and mesh attributes. This adds reusable typed
geometry to the existing cell/archive pipeline. It is still headless: no renderer,
movement, scripts, physics, saves or campaign scenario has been implemented.

| Check | Result |
| --- | --- |
| Release build | Pass, Rust 1.99.0, x86_64-pc-windows-msvc |
| Workspace checks | Formatting, all 40 tests, Clippy with warnings denied pass |
| House geometry | 207 files; 744 objects; 401 meshes; 96,437 vertices; 84,403 expanded triangles |
| Exact raw comparison | 218 files including older-stream samples; 751 objects; 403 meshes; 96,707 vertices; 84,661 triangles |
| Source fields | Float attributes match f32 bits; references and topology match exactly |
| Composed transforms | All agree within abs 1e-4 or rel 1e-5; largest absolute error 1.353e-5 |
| Deliberate mismatch checks | Float-bit change, reversed winding, changed world translation and wrong input digest all rejected |
| Full archive scan | Supported scene payloads decode in 25,729 of 25,754 supported containers |
| Successful scan totals | 133,398 objects; 67,798 geometry-data blocks; 38,958,598 vertices; 43,335,965 triangles |
| Rejected scene payloads | 25 files: 21 nonfinite-value failures, four invalid match-group-index failures |
| Independent defect checks | Raw nifly diagnostics confirm issues in all 25 rejected files |
| Source preservation | All 21 archive hashes still match the original installation baseline |

The six previously identified legacy 20.0.0.4 containers remain unsupported. The
full scan returns 1 for those and the 25 strict payload failures. Successful files
can contain unsupported blocks and branches; the scan records 12,251 such scene
edges, including animation roots. Its totals are not a rendered-world count.

## Implementation

`fallout-data::nif_scene` owns bounded payload reading, typed objects/meshes and
iterative hierarchy resolution. Its small modules separate input bounds, scene
fields, topology and transform composition. It retains raw source attributes,
flags, references and topology alongside derived transforms and triangle lists.
Cycles, multiple parents, wrong geometry-data types, invalid indices, nonfinite
floats, truncated blocks and trailing bytes are rejected. Input, block and aggregate
array budgets limit allocation. A 10,000-node fixture verifies traversal without
recursive stack growth.

Strip expansion keeps winding parity through degenerate connectors. All source
triangle counts in the successful corpus agree before connectors are removed.
The house's 91,760 connector steps illustrate why the stored count differs from
its 84,403 output triangles. One chandelier's ambient-light branch remains explicitly
unsupported. Visibility, controller evaluation, skinning and materials are not
inferred from flags or reference presence.

The separate GPL C++ oracle now offers raw factory loading. Geometry comparisons
bypass nifly's automatic cleanup, retain source data, and use its independent strip
and transform APIs. No C++ implementation or generated XML parser was added to the
Rust runtime. Reproduction commands and source boundaries are in
[scene decoding](../docs/nif-scenes.md).

## Evidence and next work

[nif-scenes.json](nif-scenes.json) records corpus totals, source failures, input
hashes and the independent comparisons. [Payload coverage](../parity/nif-scene-coverage.json)
separates the six supported block types from everything else. The parity ledger
marks this decoder as oracle-tested; there are still no accepted gameplay scenarios.
Raw model bytes and geometry reports remain under ignored `local/`.

Next work is material/texture decoding, remaining scene-node kinds, collision and
the Bevy presentation build. The 25 rejected inputs need scoped compatibility
research; Victor's shack is among them. Retail profile capture, production mount
precedence, existing compressed-source defects and SCDA decoding remain open.

The previous results are preserved in [checkpoint 02](checkpoint-02.md) and
[checkpoint 01](checkpoint-01.md). Current source and binary identities are recorded
in [source-snapshot.json](source-snapshot.json) and [verification.json](verification.json).
The working tree is still uncommitted; a source digest is not represented as a Git
revision. Continue from [NEXT_STEPS.md](../NEXT_STEPS.md).
