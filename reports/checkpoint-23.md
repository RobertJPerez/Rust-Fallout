# Implementation checkpoint 23: winning INFO topic membership

The record index now retains INFO topic-child parent labels. Canonical lookup
uses the winning source's master table and keeps moved overrides and tombstones.
Cache version 2 persists the new parent value under a distinct transform identity.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 180 passing tests and Clippy with warnings denied |
| Direct original-header comparison | All 629,788 source definitions and 628,464 winners agree |
| Winning INFO / linked topics | 28,896 / 11,994; every membership row and list agrees |
| Unresolved live topic links | 0 under the supplied inspection order |
| Cache v2 | Ten cold entries / ten warm hits; 33,438,583 encoded bytes |
| Corruption / cell regression | Damaged separate cache rejected; all 435 Doc Mitchell references reproduce |
| Independent fixtures | Four source/order scenarios, nine malformed order bundles and nine malformed source cases |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

The native reader reads original files directly under read locks. This comparison
covers header framing, master-relative keys, parent contexts and whole-record
winners; deferred bodies and record-specific retail override exceptions remain open.
Membership lists are canonical-key sorted, with no inferred retail dialogue order.

The source snapshot covers 176 source/tooling files at implementation
revision `ffe2377e516087fdfa3ded6efa9eb8995e9d6791`. Its digest is
`8273de5a51db9a8b02010e0de67a443c30807e7023fc71f697044a479e314dab`. The fresh local proof is
`local/dialogue-membership-23-verified`.

M1 remains unfinished. Canonical script identity/versioning, loaded references,
condition queries, dialogue selection and gameplay state remain open. Original
effective load order remains unmeasured. Prior GPU evidence keeps its own scope;
no retail/gameplay scenario is accepted.

See [dialogue membership](../docs/dialogue-membership.md),
[the scoped receipt](checkpoint-23-dialogue-membership.json) and
[verification](checkpoint-23-verification.json).
