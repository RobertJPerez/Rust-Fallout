# Skyrim preparation checkpoint 03

Implemented during the requested 20-minute work window starting at
2026-10-04 02:00:39 UTC (October 3, America/New_York).

The new `trace-missing` command locates absent script attachments in the verified
retail plugins, collects same-origin definition candidates, finds direct
placement/effect/enable-parent references, and identifies containing cells.
It reuses the existing census VMAD decoder through an observer API and the pinned
NV selective record reader. It verifies source hashes between passes and rejects
unsafe filenames, stale inputs and missing targets that cannot be located.

The seven absent script paths resolve to **nine source attachments**, **nine
definition candidates**, **four direct reference edges** and **three containing
cells**. All nine owning records have just one definition in this installed
corpus. No active load order or runtime winner was inferred. The trace has zero
undecoded VMAD fields, unresolved selected record keys or unresolved link indices.

| Absent script | Source evidence |
| --- | --- |
| `DLC01SoulCairnSkullPuzzleScript` | Two REFR attachments in `TestJoelDLC01` |
| `DragurEmergeScript` | One ACHR attachment in `testL` |
| `LokirsDraugrResurrection` | Two ACHR attachments in `testL` |
| `DLC1TestPhilAtronach` | NPC base used by one initially-disabled ACHR in `testGiant` |
| `dlc1testPhilVortexTrigSCRIPT` | ACTI base used by one REFR in `testGiant` |
| `DLC1testPhilVortexSCRIPT` | MGEF used by spell `dlc1testPhilVortex` |
| `DLC2BenthicLurkerFXSCRIPT` | MGEF `DLC2AbFXLurkerEffect`, used by ability `DLC2AbFXLurker` |

The first six names occur in Dawnguard; the Lurker effect occurs in Dragonborn.
None of the nine owning records has the deleted flag. Five scripts involve
the three named cells through direct attachments or placements; cell names alone
do not prove those records are unreachable. The Lurker ability deserves a next
pass through NPC SPLO/template inheritance and leveled-list/placement usage.
No missing script was fabricated or removed, and no original file was modified.

The independent Mutagen trace reader checked **16 distinct records**, **156
field/structure comparisons**, and the complete four-edge multiset across all
10 plugins: zero mismatches. It checks identities, editor IDs, flags, VMAD
presence, CELL flags and placed-reference cell containment. World ancestry,
physical offsets, recursive reachability and runtime activation are outside its
scope. Flipping one target FormID bit while keeping counts fixed is rejected:
the missing original edge and unexpected replacement are both detected.
Definition-candidate completeness is covered by the Rust scan and synthetic
override tests; the independent checker verifies returned candidates but does
not independently search for omitted definition candidates.

**32 Rust tests pass**, with formatting and Clippy warnings denied. New tests
cover source identity separation, overrides without VMAD, group-boundary expiry,
source disabled/deleted flags, stale hashes, invalid master selectors, missing
targets, malformed links, duplicate effect edges and raw enable-parent bytes.
The new .NET tool builds with warnings as errors and restores locked packages.

The full retail census after the observer refactor is byte-for-byte identical to
checkpoint 02: SHA-256
`8b23cf0b6839133b7d294cbbdd32cd2c03500b9c57e0d697192df6dd1cd209e8`.
Its eight findings remain the seven absent paths and the previously documented
Dawnguard source-index discrepancy. A successful provenance trace means the
declared trace completed; it does not clear those input findings.

Evidence: `local/missing-trace-03-final.json`, the separate trace-oracle result,
`local/negative-trace-20261004T021214Z-be59fb7e/receipt.json`, and
`local/census-20261004T021142Z-e07f5111.json`. Exact hashes and validation scope
are in [preparation-03.json](preparation-03.json) and
[source-manifest-03.json](source-manifest-03.json).

The Fallout 4 work advanced to typed Papyrus definitions and physical class/member
binding at `35ddf34`. Its new catalog was reviewed and documented for reuse; its
PEX decoder still admits only FO4's 3.9/game-2 dialect. No competing Skyrim PEX
reader or linker was added. NV and Fallout 4 working trees were left untouched.
Skyrim gameplay, original saves/native DLLs and the full paid AE corpus remain
unimplemented or untested as described in [next steps](../NEXT_STEPS.md).
