# VMAD attachments and independent evidence

`vmad::parse` consumes a complete decompressed VMAD payload. Shared NV plugin and
subrecord visitors provide the framing; no second plugin reader was introduced.
Versions 5/6 and object formats 1/2 are explicit dialects. Retail contains one v5
field and 20,373 v6 fields, all using outer object format 2. Synthetic fixtures
cover format 1 and alias-local formats that differ from their enclosing quest.

The decoder retains scripts, flags, named typed properties, nested structs/arrays,
raw plugin-local form IDs, signed aliases, padding, original string bytes, exact
float bits and boolean bytes. Empty script names retain absent flags/counts.
Every script/property/fragment/alias has a range in the decompressed VMAD payload;
plugin provenance separately retains the physical record offset and subrecord
payload offset. These offsets are not interchangeable for compressed records.

Extra bind version 3 handles INFO, PACK, SCEN, PERK, TERM and QUST suffixes.
Quest aliases use their own version and format. Unrecognized versions, presence
bits, types, trailing bytes and exhausted budgets fail with a source/byte offset.
The defaults cap a payload at 16 MiB, dynamic nodes at one million and nested
members at depth 32. Types 6/16 (variable/variable array) remain unsupported:
xEdit leaves their values unspecified and Mutagen throws for them. Neither type
occurs in this installed corpus. Unknown ordinary flags and scalar bits survive.

The sources disagree on subdivision/naming of PERK/TERM fragment indices, quest
stage fields and scene phase indices. The Rust API exposes the combined bits;
the oracle repacks the upstream fields without deciding the disputed semantics.
The agreement proves storage interpretation, not quest behavior or stage ranges.

Reproduce the independent comparison in private, fresh paths:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bins
.\local\target\debug\scan-vmad.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local/vmad-new
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-vmad-oracle.ps1
.\local\vmad-oracle-bin\Release\net10.0\vmad-oracle.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local/vmad-new local/vmad-reference-new.jsonl
.\local\target\debug\verify-vmad.exe local/vmad-new local/vmad-reference-new.jsonl local/vmad-result-new.json
```

The build script requires an unmodified checkout of the exact Mutagen revision in
sources.lock.json at `local/research/Mutagen`. It uses .NET 10 and a private NuGet
cache. Upstream C# reads every payload; the harness only exports its typed fields.
The verifier rehashes/redecodes every raw payload, checks saved Rust tokens against
that re-decoding, compares all tokens, checks missing/duplicate/extra rows and
rehashes the 16 source plugins. Strings are reversibly exported as original bytes;
comparison makes no runtime text-encoding claim. The C# reader converts bools to
0/1; all installed values match exactly. Rust preserves other byte values in tests.

Mutagen's normal identity resolver canonicalizes out-of-range master slots to the
current plugin. `RawIds` in the offline harness captures the upstream-decoded
uint at that identity boundary and gives it a reversible temporary key. It also
records what the normal resolver would change. No upstream source is patched,
and no ID is changed in Rust. Those findings are not valid loaded-world identities.
Four installed attachment fields contain IDs affected by that normalization;
their original/canonical pairs and exact record origins remain in the private
verification result.

The independent pass matches 20,374 fields, 27,652 named script entries, 78,234
properties/members, 4,870 fragment sections, 14,929 fragments, 738 phases and
3,322 aliases: 726,906 semantic tokens in total. This does not establish PEX
resolution, property assignment rules, script removal behavior or execution.

Private evidence: `local/vmad-001`, `local/vmad-reference-002.jsonl`. The earlier
reference file remains as evidence of normalization; it was not used for a passing
comparison. Pinned source licenses and oracle isolation are in docs/source-audit.md.
