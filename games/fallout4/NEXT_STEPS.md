> Integrated October 5 copy: the team is stopped at Robert's request. This copy uses `../../crates/fallout-data`; original pinned-revision notes below are historical. Read [INTEGRATION.md](INTEGRATION.md) and the root [closeout report](../../reports/closeout-2026-10-05.md) for current checks, commands and limits. Original source/research remain preserved. Root AGENTS.md and current stop control govern future work.

# Next Fallout 4 dependency slices

This workspace prepares FO4 alongside the New Vegas team. Reuse its reviewed
runtime, actor, NIF/skin, physics, world and save contracts when integration is
ready; do not recreate those systems here. FO4 remains pinned to the stable
checkpoint-45 shared-data revision until a deliberate, tested dependency update.
The latest read-only check found New Vegas main at `5a89c33` and clean. A
disposable Fallout 4 copy passed all 86 all-target tests against its latest Rust
source revision `486355d`; the following New Vegas commit changed handoff
documents only. The Fallout 4 pin and lockfile remain unchanged. See
[`reports/nv-compatibility-checkpoint.json`](reports/nv-compatibility-checkpoint.json).

1. Establish which Fallout 4 activation/profile source applies to this install.
   The new `profile` observer records explicit `plugins.txt`/`loadorder.txt` bytes
   and visible `Data` candidates without claiming game activation. The examined
   Vortex lists name many files absent from the 16-plugin `Data` tree, and that
   profile snapshot predates the current game executable/master files by about
   five months. Follow-up found two Vortex profiles and an April deployment
   snapshot for this same install. Windows' Documents known folder is redirected;
   its `My Games/Fallout4` directory contains three preserved INI inputs, with
   `Fallout4Prefs.ini` modified October 4. That is a current configuration
   candidate, not proof that the game loaded it. There is still no current
   deployment manifest. A fresh October 5 metadata-only recheck still finds the
   same April/May profile and deployment files. The historical snapshot contains none of the 272
   material texture paths missing from direct BA2 files. See
   reports/deployment-checkpoint.json, reports/profile-recheck-checkpoint.json
   and docs/configuration-observation.md.
   Inspect a current deployment/VFS or another verified plugin profile before
   classifying attachment or texture gaps. Exact INI bytes stay under ignored
   `local/`; setting values are not published.
2. Investigate the completed VMAD-to-PEX audit's unresolved references: 34 named
   script entries lack a physical class, 305 properties lack a declaration, and
   20 fragment functions lack a default-state declaration. Another 14 properties
   and 32 fragment calls belong to missing classes. Preserve these as findings;
   do not invent classes/functions or skip the source fields. `Data/Scripts` is
   absent in this installation. Establish profile/override relevance before
   treating physical findings as active-game failures. See docs/papyrus-linking.md.
3. Static callsite and branch-target inventory is now available for all 10,342
   physical PEX files: 122,952 call sites and 54,780 branch destinations, with
   source hashes and instruction offsets. All 8,706 `CALLSTATIC` sites have one
   exact-name declaration candidate in the physical corpus; 7,844 candidates are
   flagged native. Compiler-authored Fallout 4 microtests now compile static,
   method and parent calls plus a branch through the pinned Caprica compiler and
   Rust PEX parser. They verify call operand positions, five emitted argument/
   declaration type pairings, relative branch targets, FO4 STRUCTGET/STRUCTSET
   operands, and array length/read/write layouts. This checks compiler output
   consistency only. Type/arity rejection rules, runtime values, dispatch,
    continuations, struct mutation/aliasing and native host behavior remain open.
    The frozen corpus contains 1,091 array-length, 2,503 array-read, 238
    array-write, 1,455 struct-read and 181 struct-write instructions; use these
    counts to prioritize VM coverage without treating them as execution evidence.
    All 4,067,308
   definition/operand tokens are independently compared.
   VMAD variable types 6/16 and the four raw master-slot findings remain
   unresolved. See docs/papyrus-linking.md and
   reports/executable-checkpoint.json.
4. FO4-specific material groundwork extracts every direct BA2 BGSM/BGEM and
   compares bounded version-2 named fields against a pinned offline reference.
   All 643,587 field values and 32,293 texture slots match across 9,638 binary
   materials; exact float bits, boolean bytes and raw color components are kept
   in the private ledger. This does not establish shader interpretation.
   Candidate texture search retains collisions without choosing winners. A
   local gallery now previews 24 uniquely matched DDS candidates from four
   source-hash-stable archives; raw channel appearance is shown without applying
   BGSM/BGEM or renderer semantics. See docs/material-assets.md and
   reports/material-checkpoint.json. A separate 24-texture top-mip comparison
   through ImageMagick and DirectXTex found five exact pixel matches and kept all
   channel differences within one 8-bit code value. Next, verify current
   profile/VFS sources, resolve loose/deployed candidates, and compare selected
   textures with game-rendered samples to establish shader/color-space behavior.
   Integrate through NV's reviewed NIF/animation contracts when ownership is
   ready; precombined references must retain identity.
5. A raw record-header audit now covers all 5,682 records in the nine physically
   small-flag plugins: 5,514 nonzero headers use high byte `0x01`, outside the
   one-entry listed-master table, and none use the `0xFE` marker pattern. These
   values remain unassigned; the census does not apply NV's resolver or infer
   runtime ESL slots. Establish explicit FO4 activation order, master-relative
   identity and override rules. The physical master graph has 16 plugins and
   15 unique declared edges, with no physical duplicates or cycles. See
   docs/esl-identities.md, docs/plugin-dependencies.md and their reports.
   The Fallout4.esm HEDR count now has a bounded differential: Mutagen's
   independent major-record count matches Rust (1,549,276 excluding TES4), while
   its FO4 writer count is 1,661,201 vs HEDR 1,741,853. The physical walk counts
   112,381 GRUP headers (728 empty) and 1,661,657 record-plus-group headers.
   Mutagen's count adds populated typed record caches and FO4-specific CELL,
   worldspace and quest/dialogue group terms; this does not map one-to-one to
   physical GRUP headers. Keep HEDR unexplained and unmodified until its retail
   rule is verified; see docs/header-count.md.
   Follow-on workshop evidence now records 50,303 FormID slots across COBJ,
   CMPO, MISC and GLOB records. An isolated schema reader matched 37,699 nonzero
   local-ID candidates in 8,296 COBJ/CMPO/MISC records; zero CTDA references
   remain raw. This partial typed-field census does not resolve general plugin
   links, VMAD values, placed references, active slots or overrides.
   Creation archives present on disk are not evidence that their plugins are
   active or even installed. Resolve the three non-ASCII base-voice path-hash
   discrepancies against retail lookup behavior; preserve raw filename bytes
   and stored hashes throughout.
6. FO4's physical COBJ recipe catalog now covers 4,611 records across the frozen
   16-plugin corpus. Components, categories, workbench/output links, optional
   output priorities and fixed CTDA fields match the pinned Mutagen reader's
   structural fields. A second comparison checks the full 479-function metadata
   table and all three parameter type/categories for all 3,626 conditions. It
   matches 220 explicit maps and preserves 259 defaulted functions as unresolved.
   Rust carries all 479 pinned function names. The 14 explicit mappings observed in this
   corpus match for 3,609 conditions. The other 17 use `IsInInterior` and remain
   unresolved. A strict same-plugin EDID/component-index
   comparison paired all 14,916 raw component rows and quantities to Mutagen
   candidates, and a physical `IItem` census found one candidate per row. The
   raw values still have no runtime FormID or override interpretation: 74 rows
   retain a raw `0x01` high byte despite matching low 24 bits. The physical
   CMPO/MISC graph also exposes 34 scrap-item links, 33 non-null scalar-global
   links, and 1,012 MISC component links, without inferring yield. Rust now has
   bounded CMPO/MISC/GLOB decoding with malformed-width tests; a strict second
   reader matched the physical CMPO/MISC rows, all CVPA
   local-ID/count pairs and CDIX bytes. It now also inventories raw GLOB FNAM and
   FLTV fields: all 1,943 records matched pinned-schema type/presence/width
   checks, with numeric payload bits retained but not interpreted. All 33
   non-null CMPO scalar links also carry exact raw FLTV evidence for one physical
   candidate each across five GLOB keys; no value or scrap scaling is interpreted.
   A separate ledger captures 50,303 raw header/nested FormID slots; across
   8,296 COBJ/CMPO/MISC records, all 37,699 nonzero local-ID candidates match
   Mutagen, with 2,365 zeros preserved. MISC direct links and CMPO crafting
   sounds now have malformed-width tests. Numeric matches remain observations
   and assign no runtime identity. A schema-backed regression fixture now pins
   CTDA slot selection and offsets for both FormLink parameter words.
   Mutagen writer/reader fixtures now exercise float/global CTDA layouts and
   `GetVMScriptVariable` `CIS1`/`CIS2` preservation; profile-aware identity
   evidence remains open before recipe eligibility or placement behavior.
   See docs/workshop-recipes.md and reports/workshop-checkpoint.json.
7. The physical `Data` tree is fingerprinted recursively: all 107 files in the
   frozen top-level census match, and 14 nested `.bk2` videos were rehashed after
   the scan. A redirected Documents folder also supplied a current INI candidate;
   it does not prove loaded settings. Establish active plugin/VFS deployment and
   whether Fallout4Prefs.ini matches the running game's effective settings. Then
   inventory any additional loose sources and freeze the first source-driven
   interior/exterior and behavior test route. See docs/data-tree.md,
   docs/configuration-observation.md and their checkpoints.
8. A physical localization index now covers 987 `.strings`, `.dlstrings` and
   `.ilstrings` tables across 28 direct BA2 archives. Mutagen matched every one
   of 1,893,083 keys, offsets and raw value-byte hashes. The 84 archive/table
   families carry 11 or 12 path suffixes; four families have key-ID sets that
   differ from `_en`. An isolated second BA2 backend also matched all 85,632
   members in those archives (4,236,548,717 decoded bytes). These are coverage
   observations only. Verify the active language/profile, loose/VFS overrides,
   localized plugin links, text encoding, archive precedence and runtime fallback
   before using the tables in-game. See docs/localization.md and
   reports/localization-checkpoint.json.

Keep F4-0 incomplete until its independent record/asset audit and game-rendered
material comparison are delivered. F4-1 through F4-7, Papyrus execution/native APIs, rendering,
quests, settlements, power armor and saves remain open. The current implementation
is an integration-ready content prerequisite, not a gameplay checkpoint.

See `reports/checkpoint.json` for the last completed verification and exact local
evidence directory. Re-run only affected checks for incremental work; use a fresh
full proof when changing archive, framing or Papyrus contracts.
