# Installed Fallout 4 baseline

Observed installation: `G:\SteamLibrary\steamapps\common\Fallout 4`.
The executable's Windows version resource reports **1.11.240.0** for both file
and product version. Its SHA-256 is
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.

The original selected inventory consists of 108 files / 34,068,749,708 bytes:
the executable and top-level Data files. A later read-only recursive walk counted
121 physical Data files / 38,242,910,044 bytes: the same 107 direct files plus
14 nested `.bk2` videos in `Data/Video` (4,229,503,352 bytes). All 107 direct
file fingerprints match the original census, and all 14 nested files were
fingerprinted before and after the walk and rehashed at finalization. The complete
inventory is in [the recursive Data report](data-tree.md). It includes 16
plugins, 79 BA2 archives, and the base master plus all six standard DLC masters.
Creation archives and plugins are recorded independently; disk presence does
not determine activation. Saves, player settings and active load order remain
unverified.

The archives identify versions 1 (16 archives), 7 (12) and 8 (51). The loader gates
each archive's own signature/version instead of inferring the format from the
executable's label. Detailed source fingerprints stay in the private census;
the public checkpoint identifies that report's digest.

Fallout4.esm's HEDR declares 1,741,853 records/groups. The physical walk finds
1,549,276 major records excluding TES4 and 112,381 physical GRUP headers; an
independent pinned Mutagen walk matches the major-record count but its FO4
writer calculation (1,661,201) remains below HEDR. Its writer count combines
major records with typed-cache and Fallout 4-specific nested-group terms, so it
does not map one-for-one to the physical GRUP count. The stored count is retained
unchanged and never limits iteration. Exact HEDR semantics remain unresolved;
see [the count investigation](header-count.md).

Three entries in the base voice archive contain non-ASCII filename bytes whose
recomputed path hashes differ from their stored hashes. Both readers agree on
the stored hashes, exact filename bytes and extracted payloads. These are retained
as lookup/encoding findings; the physical reader does not rename or discard them.
Retail lookup behavior for those names remains unverified.

The baseline establishes available bytes and structural scope. It does not freeze
a gameplay comparison profile: language/settings, difficulty, input configuration,
save behavior and retail traces still need their own acceptance work.
