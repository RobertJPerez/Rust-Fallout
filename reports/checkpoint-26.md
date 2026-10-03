# Implementation checkpoint 26: independent compressed extraction

Every original compressed record now has independently compared stored and decoded
payload hashes. The separate offline C++ reader implements RFC format rules and
reads original source bytes directly. Strict runtime checksum policy is unchanged.

| Evidence | Result |
| --- | --- |
| Workspace | Formatting, 195 passing tests and Clippy with warnings denied |
| Compressed records | 46,696; every source identity, extent and payload digest agrees |
| Decoded data | 344,253,561 bytes |
| Existing checksum finding | One LAND record; equal decoded hash and calculated Adler |
| Strict readers | Rust and independent native reader reject the same original record |
| Valid frame fixtures | 116, including authored overlap, cross-block, empty-alphabet and window boundaries |
| Block kinds | 83 stored / 36 fixed / 33 dynamic blocks; 9,218 matches |
| Malformed frame/bundle cases | 23 rejected without a report |
| Coverage | Full bound diagnostic census and prior original-header digests agree |
| Installation | All 464 files / 9,907,238,722 bytes still match the baseline |

LAND `00150FC0`, offset `0x0B0CFF04`, decodes to 4,385 bytes with digest
`ad04d26ea0186f5aa9cc1ee3a5a0172a413eda20ed3d4586738ebeb81d4ff8c2`.
Stored Adler 1,852,853,964 differs from both calculations, 1,816,153,796.
Diagnostic agreement isolates the finding; original retail handling remains
unmeasured. No source repair or runtime compatibility exception is implemented.

The source snapshot covers 199 source/tooling files at implementation
revision `074365706bf41959c9966dda04e881882d1ad00c`. Its digest is
`02348df7804da9b0ea54c4d97c75acc79a3ce7b8fef4f158a71ce32b092dd1e2`. The fresh local proof is
`local/compressed-records-26-verified`.

M1 remains unfinished. Next: typed condition operands and subject dependencies,
then canonical event/runtime state. Original effective order, defaults, timing,
command effects and gameplay acceptance remain open. No scenario is accepted.

See [compressed records](../docs/compressed-records.md),
[the scoped receipt](checkpoint-26-compressed-records.json) and
[verification](checkpoint-26-verification.json).
