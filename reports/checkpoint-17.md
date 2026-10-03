# Implementation checkpoint 17: authored script tables and caller bindings

Rust preserves each standalone or embedded script unit and binds top-level SCDA
caller indices through that unit's own authored table. Source text is not required.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 152 passing tests and Clippy with warnings denied |
| Source | 127 source/tool/configuration files match implementation commit 7453516 |
| Independent metadata | All 80,736 units and their raw-field digests agree |
| Caller associations | All 28,463 agree: 26,139 form / 2,324 local table entries |
| Local declarations | 11,234 preserved, including 11 repeated indices / 8 conflicting repeats |
| Source findings | Three stale reference counts in empty units remain reported; diagnostic exit one |
| Malformed data | 13 independent negative bundles reject; strict LAND failure retained |
| Source safety | All 464 installed files and 9,907,238,722 bytes match the original baseline |

No compiled unit has a metadata/framing/caller issue in this inspection. All three
count mismatches belong to units without SCDA; no default table or body is invented.
Declared variable counts remain raw because neither list length nor maximum index
is a universal invariant. First-match local lookup follows the inspected source;
original retail loading of conflicting duplicates is still unmeasured.

The C++ comparison starts at Rust-extracted decoded records. Plugin extraction,
stable identity conversion, form existence, winning embedded-script identities,
loaded values and script execution are outside that comparison. Expression calls
and native arguments are the next layer. No retail/gameplay scenario is accepted.

See [script table bindings](../docs/script-bindings.md),
[the scoped receipt](checkpoint-17-script-bindings.json) and
[verification](checkpoint-17-verification.json). Earlier terrain, GPU, collision,
cache and executable-descriptor evidence keeps its own tested revision and was
not reexecuted in this checkpoint.
