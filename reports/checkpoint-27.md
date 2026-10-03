# Implementation checkpoint 27: typed condition dependencies

Rust and an independent offline reader agree on every winning CTDA field,
parameter classification and form dependency. Live query values remain unresolved.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 200 passing tests and Clippy with warnings denied |
| Winning conditions | 80,467 in 32,628 records; all original bindings agree |
| Full scan coverage | 80,627 fields filtered by fresh winners; 160 superseded fields excluded |
| Descriptor signatures | All 80,467 bind to exact executable metadata |
| Variable indices | 13,135 retained for live script state resolution |
| Form dependencies | 73,399 defined / 879 null / 1,878 explicit runtime dependencies |
| Source findings | Three findings across two original records, retained unchanged |
| Authored fixtures | 53 conditions; cold, warm and reordered bindings agree |
| Malformed inputs | 10 cases rejected by both readers without a report |
| Executable metadata | Fresh independent comparison: 640 commands, 38 events, 16 statements, 638 parameters |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

The source snapshot covers 205 source/tooling files at implementation
revision `02bb8698bb15ff7448ade1351df2884feb740155`. Its digest is
`4d13096e15b9f230af214b74bbc3dc1993d503d23ae6cf3efdf703b0532d40d3`. The fresh local proof is
`local/condition-dependencies-27-verified`.

The existing IDLE unknown subject and INFO flag/padding findings are preserved.
VATS selectors, voice types, short layouts, unknown descriptors, tombstones,
missing records and player dependencies have explicit classifications.
Target-kind acceptance, live values, query execution and retail scheduling remain
unmeasured. No scenario is accepted; M1 remains unfinished.

Next: persistent script instances and event context, followed by native save
round trips. These will establish runtime infrastructure without claiming retail
execution parity.

See [condition dependencies](../docs/condition-dependencies.md),
[the scoped receipt](checkpoint-27-condition-dependencies.json) and
[verification](checkpoint-27-verification.json).
