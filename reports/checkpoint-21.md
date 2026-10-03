# Implementation checkpoint 21: original condition fields

The loader now preserves original condition fields for typed quest, dialogue and
other record consumers. Short layouts keep absent words absent; comparison values
retain raw float bits or source FormIDs. Grouping and evaluation remain separate.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 170 passing tests and Clippy with warnings denied |
| Corpus | 80,627 conditions in 32,699 records across ten official plugins |
| Independent comparison | Every condition field, optional word, source extent and count agrees |
| Original descriptor links | All 148 condition function IDs; behavior unimplemented |
| Twenty / twenty-four / twenty-eight byte layouts | 123 / 2 / 80,502 |
| OR flags / global comparisons | 3,730 / 59 |
| Negative coverage | Ten malformed bundles rejected; three-layout fixture agrees |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

Two source findings remain unchanged: INFO `0013AC86` has low flag `0x10` and
nonzero flag padding; IDLE `00019E2F` uses subject selector `7`. Original short-layout
migration and retail handling of these fields remain unverified. The strict LAND
checksum failure remains enforced when unrelated payloads are requested.

The source snapshot covers 162 source/tooling files at implementation
revision `706c9b836a9334fe3646949092717d4b472fa905`. Its digest is
`a931b4de22f702ff903d59f130fba3cb57d841becc54848858564edc559d8b3e`. The fresh local proof is
`local/condition-fields-21-verified`.

M1 remains unfinished. Typed condition ownership, function-specific parameters,
loaded subjects, queries and native effects remain open. Prior rendering evidence
keeps its own scope; this checkpoint does not rerun or accept retail rendering.
No gameplay scenario is accepted.

See [condition fields](../docs/condition-fields.md),
[the scoped receipt](checkpoint-21-condition-fields.json) and
[verification](checkpoint-21-verification.json).
