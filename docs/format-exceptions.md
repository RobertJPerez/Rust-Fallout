# Observed vanilla format exceptions

These results describe the pinned local corpus. They do not establish tolerant retail
semantics, authorize patching the installation, or justify disabling integrity checks
globally. No source file was repaired or rewritten.

## Base plugin LAND checksum

`FalloutNV.esm`, SHA-256
`50991d36804b7d1e70df1afd7471b72f0e29d1b456ee2516a9717c002564e7c1`:

- LAND raw FormID `00150FC0`, header offset `0x0B0CFF04` (185,401,092).
- Stored record size: 4,088 bytes; declared decoded body: 4,385 bytes.
- Stored Adler-32: `6e704ecc`; calculated Adler-32: `6c404ec4`.
- Strict Rust and Python zlib checks fail. Independent raw-deflate probes produce
  the same expected output length; the framing is intact but its checksum differs.

A [TTW maintainer report](https://taleoftwowastelands.com/viewtopic.php@t=8646)
identifies this exact LAND record as a vanilla bug. That supports treating it as a known
corpus exception instead of blaming this installation. It does not by itself prove
which decoded bytes a faithful replacement should use.

The explicit `--inspect-checksum-mismatches` mode requires a valid zlib wrapper, exact
raw-deflate termination, full input consumption, exact output length, and a demonstrably
incorrect checksum. It records the mismatch and exits unsuccessfully. Strict resolution
rejects any plugin carrying recovered data. Synthetic tests cover this separation.

## Two text entries in Fallout - Misc.bsa

| Member | Data offset | Stored bytes | Strict reader | ba2 result |
| --- | --- | --- | --- | --- |
| `menus\do_not_delete.txt` | `0x9100` | 4 | Rejects incomplete zlib stream | 0 output bytes |
| `menus\s.txt` | `0x200BC` | 12 | Rejects incomplete zlib stream | 4 output bytes |

The second oracle output has SHA-256
`1a483835bb7f2b2c7b82d42798b5c32c568082cddd880f0542ce75fee3bf05ce`.
Both outcomes are retained in [the comparison report](../reports/corpus.json).
The [BSArch author's changelog](https://www.nexusmods.com/newvegas/mods/64745?tab=logs)
describes accommodating an incorrectly packed vanilla Misc archive entry by ignoring
`Z_BUFFER_ERROR`. This is relevant precedent, not evidence that arbitrary truncated
assets can be accepted.

The other 182,175 payloads are byte-equal between dream_archive and ba2. Their decoded
bytes total 10,860,465,525. Every one of the 182,177 member paths and BSA hashes was
compared, including the two entries whose payload decoding differs.

## Identity exceptions are separate from corruption

Gun Runners' Arsenal uses a self selector beyond its master count. The inspected
xEdit `FileFileIDtoLoadOrderFileID` and esplugin mappings both resolve such selectors to
the current plugin. The resolver follows that rule, retaining the original raw header
and rejecting colliding canonical identities within the same plugin.

All 4,566 SCRO links without plugin records identify `00000014`, the player reference.
[xEdit documents this as a hardcoded engine record](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/whatsnew.md).
The report now classifies it as an explicit runtime dependency. Recognition does not
instantiate the player or implement any scripts using it.

## Next compatibility decision

The scene payload scan also found 25 NIF members with numeric/index defects that
the independent raw nifly reader confirms. Twenty-one contain nonfinite colors,
UVs or translations; four contain out-of-range shared-normal match-group indices.
These are distinct from malformed triangle vertex arrays. They remain strict
decode failures. Their source digests, paths and independent field diagnoses are
recorded in [nif-scenes.json](../reports/nif-scenes.json), with scope in
[scene decoding](nif-scenes.md). No runtime repair behavior is established yet.

The material scan also found seven absolute texture paths in six DLC effect/projectile
models. Each retains an exporter prefix beginning `d:\projects\fallout\game\data\`.
The path resolver refuses these as archive identities and preserves all diagnostics
in [nif-materials.json](../reports/nif-materials.json). Whether retail strips this
prefix has not been measured; no generic absolute-path repair was added.

Capture or independently verify the retail handling of these exact malformed inputs.
If a compatibility exception is warranted, scope it to the profile, source digest,
member/record identity, and expected decoded digest; preserve provenance and diagnostics.
Do not add a global permissive-decompression switch or distribute modified retail data.
