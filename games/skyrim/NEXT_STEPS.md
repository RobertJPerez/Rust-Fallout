> Integrated October 5 copy: the team is stopped at Robert's request. This copy uses `../../crates/fallout-data`; original pinned-revision notes below are historical. Read [INTEGRATION.md](INTEGRATION.md) and the root [closeout report](../../reports/closeout-2026-10-05.md) for current checks, commands and limits. Original source/research remain preserved. Root AGENTS.md and current stop control govern future work.

# Next dependency gates

Checkpoint 04 adds a checked reverse actor-source trace and light-plugin-owned
record identities; see `reports/preparation-04.md`. The latest measured Steam
build remains 24914197 / runtime 1.7.104.0.

Checkpoint 06 adds `nif-header`, a bounded, header-only read through the existing
Skyrim BSA 105 adapter, plus `nif-list`, which safely enumerates bounded archive
index paths without extraction. One installed `Skyrim - Meshes0.bsa` skeleton has
the SSE stream tuple (20.2.0.7 / User Version 12 / stream 100). This is evidence
for the installed SE corpus only: no NIF block support or full paid AE corpus
was tested. See `reports/preparation-06.md` and `docs/nif-header-probe.md`.

Checkpoint 07 adds Skyrim `STAT` `MODL` and fixed-slot `MNAM` extraction plus
names-only matching for all ten installed plugin sources against the installed
loose files and every BSA 105 archive. The scan found 15,659 archive matches and
44 unmatched source paths; 30 source references occur in multiple archives,
with precedence unresolved. Four unmatched `Update.esm` paths are under an
Ayleid Creation Club directory; this installation does not provide them and is
not a complete paid Anniversary corpus. This does not decode model metadata,
NIF blocks, or runtime use. See
`reports/preparation-07.md` and `docs/static-model-assets.md`.

Checkpoint 08 adds a bounded SSE NIF outer-table index. It is gated to
20.2.0.7/endian-1/user-12/stream-100, leaves all block payloads opaque, and
passed synthetic tests plus six selected installed samples. It also indexed
all 18,862 `Meshes0` paths, all 3,185 `Meshes1` NIFs, and all 566 NIFs in the
four installed Creation archives. All stream-100 files passed exact block/footer
accounting; one `Meshes0` Ayleid-path member has a legacy stream-83 header and
remains unsupported. This is a Skyrim-specific adapter; the pinned NV NIF tuple
guard and shared dependency were not changed. It does not establish full SE
asset coverage, paid AE coverage, or runtime compatibility. See
`reports/preparation-08.md` and `docs/nif-header-probe.md`.

Checkpoint 09 adds `export-generic-models` for the 39 schema-allowlisted
non-`STAT` model-bearing record kinds, reusing the pinned plugin visitor and
asset-path normalizer. All ten installed plugin sources yielded 15,079 valid
model fields and no path findings. The wider source scan ties the legacy
stream-83 Ayleid NIF to an `Update.esm` `ACTI` record; this identifies a source
field and archive member but does not establish activation or runtime support.
The other four unmatched Ayleid `STAT` paths have no matching generic-model
source field in this install. See `reports/preparation-09.md` and
`docs/generic-model-assets.md`.

Checkpoint 10 adds `trace-placed-uses`, which follows an exact source `ACTI`
identity into placed `REFR NAME` fields using each plugin's declared masters.
The ten installed plugin sources contain 866,250 `REFR` records and 866,240
four-byte `NAME` fields. The Ayleid pressure-plate `ACTI` from checkpoint 09 has
zero matching placements; there are no malformed or unresolved `NAME` fields
in this bounded scan. This does not establish activation, winning overrides,
other reference types, optional-content coverage, or runtime use. See
`reports/preparation-10.md` and `docs/placed-model-uses.md`.

Checkpoint 11 adds `trace-texture-sets`, which scans the Skyrim-specific `TXST`
`TX00-TX07` fields and resolves texture-root-relative values under the Data
`textures/` root. It reuses the shared TES4 record visitor, `AssetPath`, and BSA
105 names-only index; it does not decode texture bytes or assign material
semantics. The ten installed plugin sources contain 790 `TXST` records and 2,182
path fields; 2,153 have a candidate across the 23 indexed archives and 29 remain
unmatched. The latter are exact findings for this installed content, not proof
that those files are invalid or that all paid Anniversary content is present.
See `reports/preparation-11.md` and `docs/texture-set-assets.md`.

Checkpoint 12 adds `trace-landscape-links`: Skyrim `LAND` `BTXT`/`ATXT` layers
link through `LTEX` to `TXST`, with `MNAM` material and `GNAM` grass identities.
Every physical target record is emitted once; edge candidate counts remain
joinable, and FE links stay unresolved without an explicit profile. On the ten
installed plugins it found 54,079 LAND records, 49,345 base layers, 152,844
alpha layers, and 129 `TNAM` fields. 101 layer references have no scanned LTEX
candidate. It does not decode terrain geometry, texture pixels, material data
or winners. See
`reports/preparation-12.md` and `docs/landscape-links.md`.

Checkpoint 13 extends that output with raw GRUP ancestry from the pinned shared
visitor. All 54,079 installed LAND rows have six frames, with raw kinds 0, 1,
4, 5, 6 and 9; labels remain opaque bytes, not interpreted cell coordinates.
The trace still has 101 layer links without a scanned LTEX candidate and does
not choose winners or claim terrain decoding. See `reports/preparation-13.md`.

Checkpoint 14 adds per-record LAND field-count histograms and exact payload-size
profiles. The installed source rows show `VHGT` and `VNML` together in 53,896
records and absent together in 183; sparse `VCLR` and repeated `VTXT` fields are
reported separately. This is corpus evidence for designing a Skyrim terrain
adapter, not a vertex decoder. See `reports/preparation-14.md` and
`docs/landscape-links.md`.

Checkpoint 15 joins LAND group kind 6 and kind 1 labels to physical `CELL` and
`WRLD` candidates through the owning plugin's master list. All 54,079 LAND rows
resolve both identities; candidate counts vary from one to six cells and one
to seven worlds. No winning override or cell-coordinate mapping is selected.
See `reports/preparation-15.md` and `docs/landscape-links.md`.

Checkpoint 16 adds a bounded structural `CELL XCLC` adapter to the landscape
trace. It preserves signed grid coordinates for the observed eight-byte form
and the optional flags/reserved-byte tail in the twelve-byte form; unsupported
lengths, cells without XCLC, and repeated XCLC fields remain explicit. Opaque
CELL fields also get bounded tag/payload-length profiles to guide later Skyrim
adapters. It does not convert coordinates or claim runtime cell behavior. See
`reports/preparation-16.md` and `docs/landscape-links.md`.

Checkpoint 17 adds source-preserving `CELL XCLW` water-height evidence. Four-byte
values retain exact IEEE 754 bits and hashes; non-finite values do not become
JSON numbers, and unsupported lengths remain visible. Absence is not replaced
with an inferred WRLD default. The installed profile has one XCLW field on
every scanned CELL. Candidate bits disagree for 33 LAND-referenced cell source
keys; all candidates remain separate pending an explicit order. Runtime water
selection remains unverified. See
`reports/preparation-17.md` and `docs/landscape-links.md`.

Checkpoint 18 adds bounded raw evidence for CELL XCLL lighting fields. The
installed corpus contains 64-byte and 92-byte forms; the trace keeps exact
hashes and previews on each physical CELL row and leaves all lighting members
opaque. No lighting or rendering behavior is inferred. See
`reports/preparation-18.md` and `docs/landscape-links.md`.

Checkpoint 19 links CELL XCLR entries to physical REGN candidates through the
existing FormID resolver, retaining whole-field hashes and visible partial
tails under a global entry budget. The installed trace has 83,516 entries and
30 source-key-resolved links without a scanned candidate, all to one
Skyrim.esm-owned key. Keep those unresolved; they do not prove the referenced
form is invalid. See `reports/preparation-19.md` and `docs/landscape-links.md`.

Checkpoint 20 adds Skyrim-specific CELL `XCCM` → `REGN`, `XLCN` → `LCTN`, and
`XWEM` asset evidence to the same trace. XWEM requires a bounded NUL-terminated
`Data`-rooted path, then reuses shared `AssetPath` normalization and loose/BSA
name lookup. The installed ten-plugin profile has 109 fields across 16 unique
paths: 106 references match indexed BSA names, none match loose files, and
three Dawnguard references to `textures/cubemaps/lavacubemap_e.dds` have no
candidate among the scanned loose files or named archive entries. No archive
indexes failed and no XWEM fields were malformed. The missing candidate is not
classified as invalid; the profile has four installed Creation pairs and
does not establish full paid Anniversary coverage. The trace also corrects
`XCCM` to its Skyrim REGN target and finds scanned targets for every non-null
CELL context link. See `reports/preparation-20.md` and
`docs/landscape-links.md`.

Checkpoint 21 extends the same Skyrim-specific graph from CELL `XLCN` links to
`LCTN` `PNAM` parent-location links. The ten-plugin source scan contains 841
physical LCTN rows and 811 four-byte parent links; every source key has one or
more scanned physical LCTN candidates, but 428 links have multiple candidates
(up to five), so no override or winning hierarchy is selected. Thirty LCTNs
have no PNAM field. The adapter keeps all other LCTN fields as hashed bounded
evidence, with 6,312 known fields and no unrecognized LCTN tags in this
installed corpus. This records only the on-disk parent graph; it does not
establish runtime containment, location ownership or quest behavior. See
`reports/preparation-21.md` and `docs/landscape-links.md`.

Checkpoint 22 adds a bounded player movement source trace: the exact `Player`
`NPC_` editor ID to `RNAM` race candidates, then race movement defaults and
`MTYP` links to physical `MOVT` records. On the installed ten-plugin profile,
one Player record points to two physical `NordRace` rows (`Skyrim.esm` and
`Update.esm`); both remain candidates. The 239 encoded race-to-movement links
all have scanned candidates, and no movement link requires an active profile.
The two `NordRace` candidates carry no encoded movement links or race speed
override, which remains an explicit source omission rather than an inferred
default. The trace preserves raw IEEE-754 values, including the version-28
`MOVT SPED` slot change, without converting units or claiming runtime
locomotion behavior. This installed profile has four Creation plugins and does
not establish full paid Anniversary coverage. See
`reports/preparation-22.md` and `docs/movement-profile.md`.

Checkpoint 23 adds an eight-byte PEX dialect-prefix observation to the
installation census. It uses the pinned archive backend and shared loose-file
source hashing, recognizes the observed Skyrim big-endian 3.2/game-1 tuple, and
keeps other tuples, unexpected magic and truncated prefixes as findings. It
does not decode PEX bodies or implement Papyrus. Synthetic archived and loose
fixtures pass in the 76-test suite. The installed census has not been rerun
after tightening the tuple check, so no refreshed installed PEX counts are
claimed. See `docs/script-provenance.md`.

1. Preserve the verified 1.7.104.0 baseline and its seven absent script references.
   The source trace now identifies nine attachments, no additional installed
   definitions of those owners, four direct edges and three containing cells.
   The two attached missing-script magic effects point to two spells. No NPC
   `SPLO`, `LVSP`/`LVLN` `LVLO`, `TPLT` descendant or `ACHR NAME` source path to
   those spells was found in these ten plugins; the independent reader agrees.
   This does not rule out quests, Papyrus-created actors, runtime grants, other
   profiles or optional Creation content. The current profile has four included
   Creation plugins and three light plugins, not the complete paid AE corpus.
   Test-cell names and source disabled flags do not prove runtime reachability.
   Rerun `tools/inspect-install.ps1` when relevant code or inputs change; retain
   the older reports. Executable version and optional content are separate.
2. Resolve the Dawnguard `Collision` reference described in
   `docs/binding-contract.md` against original runtime behavior before choosing
   any fallback policy. All 10,178 VMAD tails now decode; the independent reader
   matches 24,630 of 24,631 complete bindings, with this one raw-ID discrepancy
   kept as a failure. All 14,302 archived PEX payload hashes independently match.
   Preserve unsupported formats and continue to require independent comparison
   when expanding the supported corpus.
3. Consume the Fallout 4 prep project's Papyrus parser through an agreed shared
   dialect API when its Skyrim decoding is tested. Use this project's PEX hashes
   and source-addressed VMAD binding exports as inputs. Add class/import/property linking and explicit
   unresolved natives without creating a second bytecode reader.
4. Have the NV coordinator review a shared Skyrim identity namespace and existing
   load-order primitives. Physical records owned by a light plugin now retain that
   plugin's local ID when the HEDR version validates it. A Skyrim-owned explicit
   profile mapper assigns separate full/light slots and checks masters/order, but
   the standard Windows profile paths were empty, so no order was available to
   verify. See [the profile mapping contract](docs/profile-mapping.md). Cross-file
   FE references in plugin-file context and winning override chains remain unresolved. Inventory
   and installed-file presence do not establish activation.
5. Expand validation of the Skyrim SSE outer-table adapter from
   `docs/nif-header-probe.md` across base and installed Creation archives,
   retaining per-file parse failures and tuple variants. Synthetic fixtures
   and all four installed Creation archives pass (566/566 NIFs), plus all 3,185
   `Meshes1` NIFs and 18,861/18,862 `Meshes0` NIFs. The remaining Meshes0 file
   is visibly stream-83 and remains unsupported. Six named samples also pass;
   no block payload is parsed. Use the `STAT`, generic-model, `TXST` and
   landscape-link source traces in checkpoints 07, 09, 11, 12, 13, 14, 15, 16, 17, 18 and 19 with local
   unmatched/overlap ledgers
   to prioritize assets; rerun applicable source traces after each optional
   Creation plugin/archive is actually installed. Before adding a second payload converter,
   compare the pinned external NIF-conversion candidate in `sources.lock.json`
   against this exact stream-100/stream-83 corpus and owner-aware plugin IDs;
   its support remains unverified.
   Any later payload adapter needs an independently verified Skyrim block
   schema, especially for geometry, shader/material, skinning, animation and
   Havok. NV NIF versions and payload decoders do not establish Skyrim support.
6. Establish an owned retail baseline and a measured original new-game/Helgen route
   with Papyrus events, actors, collisions, voiced dialogue and cold Continue.
   Track Dawnguard, Hearthfire, Dragonborn and each installed Creation separately.

No gameplay scenario or native DLL/save compatibility is currently accepted.
