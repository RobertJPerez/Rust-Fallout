# Implementation checkpoint 16: vanilla command and event signatures

The offline Rust inspector reads the exact fingerprinted New Vegas executable
and preserves command names, IDs, parameter metadata and source file offsets.
It never loads or calls original executable code.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 147 passing tests and Clippy with warnings denied |
| Source | 121 source/tool/configuration files match implementation commit 558d91e |
| Direct-source comparison | All 640 commands, 38 events and 16 statements match original C++ |
| Parameters | All 638 parameter descriptor occurrences match independently |
| Authored usage | 217 top-level command IDs and 33 event IDs bind to metadata |
| Conditions | All 148 function IDs / 80,627 occurrences bind to vanilla evaluator descriptors |
| Fingerprint guard | Both readers reject a changed executable copy without a catalogue |
| Source safety | All 464 installed files and 9,907,238,722 bytes still match baseline |

Native effects, return types, errors, caller rules, condition evaluation and event
scheduling are unimplemented. Handler presence is metadata, not runtime support.
Expression calls are still outside the command count. The catalogue is gated by
the exact executable SHA-256, not its displayed version alone.

See [command metadata](../docs/command-catalogue.md),
[the scoped receipt](checkpoint-16-command-catalogue.json) and
[verification](checkpoint-16-verification.json). Checkpoint 15 retains its own
compiled-body/header proof. Earlier GPU, terrain, collision and cache evidence is
unchanged and was not reexecuted in this metadata checkpoint. No retail or
campaign/gameplay scenario is accepted.

Next: preserve each embedded script's local/reference tables, validate source
bindings and decode expression/native operands before building verified execution.
