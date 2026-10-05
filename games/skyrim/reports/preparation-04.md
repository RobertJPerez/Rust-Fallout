# Skyrim preparation checkpoint 04

This checkpoint extends the missing-script provenance trace through Skyrim actor
spell fields, templates, leveled spells/NPCs, and placed actors. It also binds
records owned by an ESL to that source file's validated local-ID range while
keeping cross-file light references unresolved until a load-order map exists.

The measured Steam installation is build `24914197`, executable `1.7.104.0`, with
10 plugins and 23 BSA archives. All 23 archives report version 105. The installed
set contains the five base masters, `_ResourcePack.esl`, and four Creation plugins:
`ccBGSSSE001-Fish.esm`, `ccBGSSSE025-AdvDSGS.esm`, `ccBGSSSE037-Curios.esl`, and
`ccQDRSSE001-SurvivalMode.esl`. This is the installed profile we measured, not the
full paid Anniversary content set. The Steam state is complete. The census hash
is byte-identical to the previous verified census, and the same eight input
findings remain: seven absent script paths and the documented Dawnguard raw
FormID discrepancy.

The seven absent paths attach to nine records, with nine same-origin definition
candidates, four direct source edges, and three containing cells. Two of the
attached records are missing-script magic effects; their source `EFID` links lead
to two spells. In the ten installed plugins, no NPC `SPLO`, NPC `TPLT` descendant,
`LVSP`/`LVLN` `LVLO` entry, or placed `ACHR NAME` field points into those spell
keys. The resulting actor-path graph has zero actor definitions, zero links, and
zero unresolved fields or source identities. This describes the scanned plugin
sources. It does not
rule out quest logic, Papyrus-created actors, runtime spell grants, another
profile, or other Creation content, and it does not establish gameplay reachability.

The independent Mutagen 0.54.4 reader scanned all ten plugin files and agreed on
all four direct edges. Across 176 source comparisons it found zero mismatches; it
also found zero actor-path edges or candidate definitions for the two spell keys.
The mutation control changes one direct FormID bit without changing graph counts;
the checker rejects it with the expected missing-edge and unexpected-edge pair.

The light-plugin regression creates a synthetic `.esl` fixture. It verifies that
a light plugin's own record and containing-cell IDs map to that source file only
when the header's HEDR range permits them. The same raw `FE` bits in a cross-file
`NAME` link remain unresolved without active light-slot state. Skyrim's leveled
entry adapter accepts the typed 12-byte `LVLO` layout, preserves every byte, and
reads its FormID at offset four. Unsupported sizes remain explicit findings.

Rust verification passed 34 tests, formatting, warnings-denied Clippy, and the
binary build. The independent .NET oracle builds with zero warnings and its
unaltered/mutated trace control passes. The retail census, trace JSON, and detailed
oracle receipts remain under ignored `local/`; original game files were read-only.
No active plugin order, override winners, paid AE corpus, campaign, save, native
DLL, or gameplay behavior is accepted by this checkpoint.

See [the trace contract](../docs/script-provenance.md),
[shared integration notes](../docs/shared-integration.md), and
[next dependency gates](../NEXT_STEPS.md). The explicit Skyrim profile mapper
and current source manifest are in the follow-on
[profile preparation checkpoint](preparation-05.md).
