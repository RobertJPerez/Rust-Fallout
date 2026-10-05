# Fallout 4 preparation

A Rust content adapter built alongside the active New Vegas engine. It reuses
the engine's existing plugin framing, strict record decompression and asset path
validation through a pinned dependency. Fallout 4-specific work lives here for
later integration. This is a headless inspection tool, not a playable Fallout 4.

The [verified checkpoint](reports/checkpoint.json) covers 16 plugins, 79 archives,
and 10,342 archived Papyrus files from the installed 1.11.240.0 build. Independent
readers agree within the documented structural/payload comparison scope. Three
voice-name hash discrepancies and one master-header count discrepancy remain
explicit findings. Gameplay implementation and acceptance are still open.

Implemented paths:

- Read-only installation inventory, SHA-256 fingerprints, plugin/master census,
  record/subrecord counts and VMAD header inventory.
- FO4-specific Fallout4.esm HEDR investigation: a second physical group census
  and a pinned Mutagen major-record/GetRecordCount comparison, with the remaining
  writer-count difference kept explicit; see [HEDR count evidence](docs/header-count.md).
- BA2 version 1/7/8 GNRL and DX10 access through the same archive library already
  selected by NV, with additional index/name/chunk/member allocation limits.
- Fallout 4 PEX 3.9/game 2 decoding: string references, debug framing, classes,
  structs, variables, properties, states, functions, instruction operands and
  native signatures. Original bytes and offsets remain available.
- An isolated upstream PEX reader comparison, including 4,067,308 definition and
  operand values across all 10,342 installed archived scripts.
- VMAD attachments, typed properties/recursive structs, quest aliases and six
  record fragment layouts. All 20,374 installed fields and 726,906 semantic tokens
  match an isolated independent reader. See [VMAD evidence](docs/vmad.md).
- Static VMAD-to-PEX binding with physical source provenance, explicit duplicate
  candidates and bounded parent/property/default-state function lookup. This
  does not choose an active load order or execute scripts.
- Static Papyrus call/branch inventory across the frozen PEX corpus, with exact
  source locations, branch instruction indices and unselected `CALLSTATIC`
  declaration candidates. Method/parent dispatch and native host implementations
  remain open. Compiler-authored Fallout 4 fixtures now check static, method and
  parent call operands, five argument/declaration type pairings, relative branch
  targets, struct-member opcodes and array access layouts through the Rust
  parser. The frozen corpus has 1,091 array-length, 2,503 array-read, 238
  array-write, 1,455 struct-read and 181 struct-write instructions. These are
  physical counts, not runtime claims. See
  [Papyrus linking](docs/papyrus-linking.md).
- Raw header-FormID observations for physically small-master plugins, with source
  fingerprints and no assigned ESL runtime/load-order slots; see
  [ESL identity observations](docs/esl-identities.md).
- Physical TES4 master-dependency graph with explicit missing/ambiguous candidates
  and cycle findings; it does not infer plugin activation or winner order. See
  [plugin dependency audit](docs/plugin-dependencies.md).
- A Fallout 4 profile-file observer that hashes explicit `plugins.txt` and
  `loadorder.txt` inputs, preserves literal activation markers, and reports
  plugin filenames absent from the visible `Data` tree without inferring winners.
  A separate metadata-only recheck confirms the known profile/deployment files
  remain stale and does not publish their contents; see
  [current profile evidence](reports/profile-recheck-checkpoint.json).
- A bounded profile INI capture that resolves the Windows Documents known folder,
  preserves only `Fallout4.ini`, `Fallout4Prefs.ini` and `Fallout4Custom.ini` under
  ignored `local/`, and publishes no setting values; see
  [configuration observation](docs/configuration-observation.md).
- A recursive, before-and-after fingerprint inventory of the physical `Data`
  tree, including its 14 nested `.bk2` videos. It checks the frozen top-level
  census and still does not infer active profiles or VFS deployment; see
  [Data tree evidence](docs/data-tree.md).
- A Fallout 4 workshop catalog for COBJ recipes plus raw CMPO/MISC/GLOB scrap
  structures. Its separate FormLink ledger records 50,303 header/nested slots;
  the isolated Mutagen comparison matched 37,699 non-null nested local-ID
  candidates across 8,296 COBJ/CMPO/MISC records. CTDA slots remain raw,
  unknown parameter types stay unresolved, and no condition or settlement
  behavior is executed. See [workshop preparation](docs/workshop-recipes.md).
- An FO4 BGSM/BGEM version-2 field decoder checked against a pinned offline
  reference across 9,638 binary materials: 643,587 named field values and 32,293
  texture slots matched. Float and source color bits remain available in a local
  ledger; shader meaning and rendering remain open. A candidate-only texture-name
  search retains every direct-BA2 match without choosing winners. A local gallery
  previews 24 uniquely matched DDS candidates from four fingerprinted archives;
  A second decoder compared all 24 top mips: five were byte-exact and all stayed
  within one 8-bit code value per channel. Game-renderer parity remains open. See
  [material asset preparation](docs/material-assets.md).
- A physical BA2 localization-table inventory with bounded `.strings`,
  `.dlstrings` and `.ilstrings` parsing, plus an all-key offset/value-byte
  comparison against the pinned Mutagen reader. It preserves locale path labels
  without selecting a language or decoding translations; see
  [localization preparation](docs/localization.md).

On this machine, use the existing Rust 1.99.0 compiler with private build storage:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --locked --jobs 1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bins
.\local\target\debug\fallout4-prep.exe --help
.\local\target\debug\fallout4-prep.exe census 'G:\SteamLibrary\steamapps\common\Fallout 4' --full-hash
```

The dependency uses a local Git URL pinned to checkpoint 45. It does not follow
uncommitted NV edits or branch movement. A different machine needs a clone of
that repository at `G:\Rust-Fallout`, or an intentional edit of the Git URL while
retaining the revision. For normal Rust installations, `cargo` works without the
machine-specific wrapper. Integration into NV should replace this dependency
declaration with its workspace dependency; do not copy the shared parser.

JSON goes to stdout and progress to stderr. Exit 0 means the requested structural
scan completed, 2 means the emitted census contains failures/missing masters, and
1 means the command could not complete. `--full-hash` hashes all selected files;
without it only executable/plugin bytes are hashed. Neither mode enumerates
subdirectories or determines which Creation Club/mod plugins are activated.

`extract-pex` creates a **new** directory outside the source installation and
publishes `manifest.json` last. An interrupted directory is incomplete; choose
a fresh output directory on retry. It never overwrites prior evidence.
Do not update/truncate source archives while mmap readers are open.

`extract-materials` likewise creates a new directory outside the retail
installation, hashes source archives around extraction, and writes its
completion marker last. The separate material reference and texture candidate
commands are documented in [material asset preparation](docs/material-assets.md).

See [architecture](docs/architecture.md), [source audit](docs/source-audit.md),
[profile observation](docs/profile-observation.md),
[profile checkpoint](reports/profile-checkpoint.json),
[current profile recheck](reports/profile-recheck-checkpoint.json),
[configuration checkpoint](reports/configuration-checkpoint.json),
[deployment checkpoint](reports/deployment-checkpoint.json),
[validation](docs/validation.md), the [Papyrus checkpoint](reports/papyrus-checkpoint.json),
the [executable inventory checkpoint](reports/executable-checkpoint.json), the
[FormID checkpoint](reports/formid-checkpoint.json), the
[plugin dependency checkpoint](reports/plugin-dependency-checkpoint.json), the
[workshop recipe checkpoint](reports/workshop-checkpoint.json), the
[localization checkpoint](reports/localization-checkpoint.json), the
[recursive Data checkpoint](reports/data-tree-checkpoint.json), the
[HEDR count checkpoint](reports/header-count-checkpoint.json) and
[next work](NEXT_STEPS.md).
