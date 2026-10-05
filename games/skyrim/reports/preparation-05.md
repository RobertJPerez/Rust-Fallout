# Skyrim preparation checkpoint 05

This checkpoint adds a Skyrim-owned active-plugin slot mapper and corrects the
inventory distinction between the TES4 light-plugin flag and the `.esl` suffix.
It preserves the pinned shared parser/archive dependency and adds no Skyrim entry
to Fallout's shared `ProfileId`.

The new `map-profile` command accepts a bounded, versioned JSON list of active
plugins in caller-supplied order. It inspects only those plugins, fingerprints
their files and the order input, checks the HEDR dialect, direct master presence
and ordering, duplicate names and full/light slot capacities, and reports the
independent full and light slot spaces. The public mapper can encode source
plugin/local identities to candidate runtime IDs and decode already runtime
encoded full/light IDs. Synthetic tests cover 1.70 and 1.71 light-local ranges,
slot separation, master order, duplicate entries and the important case where an
`.esl` suffix without the TES4 light flag remains in the full-plugin space.

No active order was available for the installed game. The Windows Known Folder
game-profile directory, LocalAppData game-profile directory and expected Vortex
Skyrim profile directory contained no `plugins.txt` or `loadorder.txt`. This
does not rule out a profile in another mod manager or custom location. The retail
profile mapper was therefore not run. `map-profile` does not parse or discover
manager-specific files, certify that the game consumed its input, or choose
winning overrides. Cross-file `FE` values in plugin-file context remain
unresolved by `trace-missing` until a measured profile and source encoding are
connected to that pass.

The refreshed Steam baseline is build `24914197`, executable `1.7.104.0`, with
10 plugins and 23 BSA archives (all v105). Three installed plugins carry the
TES4 light flag: `_ResourcePack.esl`, `ccBGSSSE037-Curios.esl` and
`ccQDRSSE001-SurvivalMode.esl`. Two use HEDR 1.71; the other eight use 1.70.
The four observed Creation plugins are not the full paid Anniversary corpus.
The new census records eight blocking findings (seven absent script paths plus
the documented Dawnguard raw FormID discrepancy); its retail inputs were
read-only.

The current missing-script trace still finds seven absent paths, nine
attachments, nine same-origin definitions, four direct edges and three
containing cells. Its two attached missing-script magic effects lead to two
source spells, with no NPC/leveled-list/placed-actor actor path in these ten
plugin sources. Mutagen.Bethesda.Skyrim 0.54.4 independently matched all four
direct edges and all 177 reported comparisons with no mismatch. The mutation
control flips one direct FormID bit and is rejected with the expected two edge
mismatches. These are source-data checks, not proof of game activation or
gameplay reachability.

Formatting, warnings-denied Clippy, binary build and all 39 Rust tests passed.
The end-to-end synthetic profile CLI test passed. The independent .NET trace
oracle built with zero warnings. Original installation files remain unchanged;
retail reports and oracle artifacts are under ignored `local/`.

See the [explicit profile contract](../docs/profile-mapping.md),
[script provenance](../docs/script-provenance.md),
[shared integration boundary](../docs/shared-integration.md), and
[remaining work](../NEXT_STEPS.md). The machine verification record and source
manifest are [preparation-05.json](preparation-05.json) and
[source-manifest-05.json](source-manifest-05.json).
