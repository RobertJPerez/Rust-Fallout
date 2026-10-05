# Rust Skyrim preparation

Working Skyrim Special Edition / Anniversary Edition content preparation built on
the existing New Vegas project's data libraries. This is a read-only loader and
dependency inventory, not yet a playable Skyrim runtime.

[Content checkpoint](reports/preparation-02.md): all 10 installed plugins and 23
archives are scanned; all 10,178 VMAD fragment/alias tails decode. Independent
readers match all 14,302 archived script payloads and 24,630 of 24,631 complete
VMAD bindings. One Dawnguard raw-reference discrepancy and seven absent script
references remain explicit.

[Script-provenance checkpoint](reports/preparation-05.md) traces the absent
script effects through spell, actor, template and leveled-list source fields.
The two effect spells have no authored NPC/leveled-list/placed-actor paths in the
measured ten-plugin profile; an independent Skyrim reader agrees. The report also
keeps cross-file `FE` references unresolved without a verified light-slot map. See
the [trace command and its scope](docs/script-provenance.md).

[NIF preparation checkpoint](reports/preparation-06.md) adds bounded NIF path
indexing and header inspection through the shared Skyrim BSA 105 reader. The base
skeleton and one sample from each of four installed Creation archives expose the
SSE stream-100 header tuple. This does not decode mesh blocks or establish a full
paid Anniversary corpus; see the [NIF probe limits](docs/nif-header-probe.md).

[Static model checkpoint](reports/preparation-07.md) adds Skyrim `STAT`
model/LOD path extraction and a names-only trace across all ten installed
plugins and the 23 installed BSA indexes. It matched 15,659 of 15,703 source
paths without selecting archive winners; 44 remain unmatched and 30 references
span multiple archives. Four unmatched `Update.esm` source paths name Ayleid
Creation Club directories, but the installed paid Anniversary corpus remains
incomplete. See the [static model asset evidence contract](docs/static-model-assets.md).

[SSE NIF table checkpoint](reports/preparation-08.md) adds a Skyrim-specific,
exact-tuple outer-table index for stream 100. It reports block types, byte spans,
string/group tables and roots while preserving payload bytes as opaque; synthetic
fixtures, six selected installed assets, all 3,185 `Meshes1` NIFs, all 566 NIFs
in the four installed Creation archives, and 18,861/18,862 `Meshes0` NIFs pass
exact footer accounting. The remaining `Meshes0` member has a legacy stream-83
header and stays unsupported. The paid Anniversary corpus remains incomplete;
this is not a runtime compatibility claim.

[Generic model checkpoint](reports/preparation-09.md) adds source-field
extraction for 39 schema-confirmed non-`STAT` record kinds. It reuses the pinned
plugin reader and path normalizer and does not duplicate shared parsing. The
ten installed plugins yielded 15,079 normalized model fields; one connects the
legacy Ayleid pressure-plate NIF to an `Update.esm` `ACTI` source record. Its
stream-83 compatibility remains unknown. See the
[generic model evidence contract](docs/generic-model-assets.md).

[Placed-use checkpoint](reports/preparation-10.md) adds a source-only trace from
an exact `ACTI` FormID to matching placed `REFR NAME` fields. It uses declared
plugin masters to preserve record ownership and retains cell context. Across the
ten installed plugins, the Ayleid pressure-plate activator has no matching
placed reference; this says nothing about optional content, runtime activation,
or full Anniversary Edition coverage. See the
[placed-use evidence contract](docs/placed-model-uses.md).

[Texture-set checkpoint](reports/preparation-11.md) adds a source-only census of
Skyrim `TXST` texture slots. It resolves each observed texture-root-relative
field to a Data-root `textures/` path, then checks loose files and all 23
installed BSA 105 name indexes. The ten installed plugins contain 790 texture
sets and 2,182 path fields; 29 fields have no candidate in this incomplete SE
content set. No texture payloads, material meanings, archive winners or runtime
use are inferred. See the
[texture-set asset evidence contract](docs/texture-set-assets.md).

[Landscape-link checkpoint](reports/preparation-12.md) adds Skyrim `LAND`
`BTXT`/`ATXT` layer links through `LTEX` to `TXST`, plus `MATT` and `GRAS`
identities. It retains every matching physical candidate without picking
overrides; unresolved light-plugin references and the 101 installed layer
references without a scanned `LTEX` candidate remain visible. Join its `TXST`
physical IDs with the texture-set trace to continue to normalized texture
paths. Terrain geometry and pixel payloads remain unparsed. See the
[landscape-link evidence contract](docs/landscape-links.md).

[Landscape hierarchy checkpoint](reports/preparation-13.md) extends that trace
with the shared reader's raw GRUP ancestry for each LAND record. All 54,079
installed LAND rows have six parent-group frames, including raw kinds 0, 1, 4,
5, 6 and 9. Labels remain bytes; no worldspace, cell or coordinate meaning is
assigned yet. The group path is ready for a separately verified Skyrim
hierarchy adapter.

[Landscape field-shape checkpoint](reports/preparation-14.md) adds per-record
occurrence histograms for each LAND field tag. It distinguishes sparse height,
normal and color fields from repeated `VTXT` chunks, while retaining exact
payload-size evidence. This bounds later Skyrim terrain parsing work without
decoding vertex data or claiming rendering behavior.

[Landscape cell-context checkpoint](reports/preparation-15.md) resolves LAND
group kind 6 and kind 1 labels through each plugin's master list to physical
`CELL` and `WRLD` candidates. All 54,079 LAND rows have candidates for both
labels; candidate multiplicities are retained rather than choosing an
override. Cell coordinates and cell bodies remain unparsed.

[Landscape cell-grid checkpoint](reports/preparation-16.md) inspects `CELL`
`XCLC` payloads using the pinned Skyrim field layout. It retains signed grid
coordinates, the optional flag/reserved tail, and raw evidence for unsupported
shapes, repeated fields, and cells without `XCLC`. It does not convert grid
coordinates or infer runtime streaming behavior.
Opaque CELL fields also receive bounded tag and payload-length counts for
future adapter prioritization: the installed scan profiled 400,969 fields
across 25 tags without decoding the opaque payloads.

[Cell water-height checkpoint](reports/preparation-17.md) carries that bounded
CELL adapter forward for XCLW. It preserves exact float bits, hashes and raw
bytes, reports non-finite classes without placing them in JSON numbers, and
keeps missing fields explicit instead of filling defaults. In the installed
profile all 77,337 CELL records have one four-byte value; 33 LAND-referenced
cell identities have differing physical candidate bit patterns, so the trace
keeps every candidate for a later explicit order mapping.

[Cell lighting checkpoint](reports/preparation-18.md) attaches raw XCLL field
length, hash and preview evidence to each physical CELL. It recognizes the two
installed sizes, 64 and 92 bytes, without decoding lighting members.

[Cell region checkpoint](reports/preparation-19.md) links complete CELL XCLR
entries to physical REGN candidates using the existing FormID resolver. Whole
fields keep hashes and bounded previews; incomplete trailing bytes stay visible.
The installed profile contains 83,516 entries, of which 83,486 have scanned
REGN candidates. Thirty entries point to one Skyrim.esm source key without a
scanned REGN candidate; that finding is retained without declaring the target
invalid or selecting another region.

The Skyrim-only `map-profile` command accepts an explicit ordered active-plugin
list and maps full and light runtime slots after checking plugin headers and master
ordering. The standard Windows profile paths were empty, so the command does not
discover or certify the game's current order. See the
[profile mapping contract](docs/profile-mapping.md).

The current command frames every installed plugin record using the shared parser,
checks Skyrim plugin metadata and light-plugin ID ranges, decodes VMAD v4/v5 script
attachments, property values, quest aliases and record-specific fragments,
indexes BSA 105 archives through the existing
archive backend, and decompresses/hashes their PEX members. It joins attachment
names to available archived and loose scripts and reports missing dependencies or
ambiguous candidates without guessing an active load order.

## Build and run on this workstation

The shared dependency is pinned to verified NV checkpoint 45 at
`9265ef63f714cdf01ff02028640a582be9639c7a`. Cargo obtains its own checkout from
`G:\Rust-Fallout`; active NV working files are not inputs. The existing Rust 1.99.0
compiler is reused with private caches and output. This local Git URL deliberately
requires that repository; it is not a portable public distribution yet.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --locked --jobs 1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/inspect-install.ps1
```

The wrapper waits for no downloads, changes no install settings, and checks Steam's
installation state before opening retail files. Rerun it after an update when you
need to refresh the measured corpus. For a non-Steam install, pass an explicit
installation directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/inspect-install.ps1 -Install 'D:\Games\Skyrim Special Edition'
```

Direct executable usage:

```powershell
.\target\debug\skyrim-prep.exe inspect --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' --output 'local\census-new.json'
.\target\debug\skyrim-prep.exe inspect-plugin --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim.esm' --output 'local\plugin-new.json'
.\target\debug\skyrim-prep.exe export-bindings --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim.esm' --output 'local\bindings-new.jsonl'
.\target\debug\skyrim-prep.exe export-stat-models --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim.esm' --output 'local\stat-models.jsonl'
.\target\debug\skyrim-prep.exe export-generic-models --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Update.esm' --output 'local\generic-models.jsonl'
.\target\debug\skyrim-prep.exe trace-placed-uses --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' --target-plugin Update.esm --form-id 0x01003206 --plugin Skyrim.esm --plugin Update.esm --plugin Dawnguard.esm --plugin HearthFires.esm --plugin Dragonborn.esm --output 'local\placed-uses.json'
.\target\debug\skyrim-prep.exe trace-stat-assets --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim.esm' --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Update.esm' --output 'local\stat-assets.jsonl'
.\target\debug\skyrim-prep.exe trace-landscape-links --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim.esm' --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Update.esm' --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Dawnguard.esm' --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\HearthFires.esm' --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Dragonborn.esm' --output 'local\landscape-links.jsonl'
.\target\debug\skyrim-prep.exe nif-list --archive 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\ccBGSSSE037-Curios.bsa' --limit 64 --output 'local\curios-nifs.json'
.\target\debug\skyrim-prep.exe nif-list --archive 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim - Meshes1.bsa' --offset 512 --limit 512 --output 'local\meshes1-nifs-page-2.json'
.\target\debug\skyrim-prep.exe nif-header --archive 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim - Meshes0.bsa' --member 'meshes\actors\character\character assets\skeleton.nif' --output 'local\skeleton-header.json'
.\target\debug\skyrim-prep.exe nif-index --archive 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim - Meshes1.bsa' --member 'meshes\LoadScreenArt\LoadScreenMRaltar01.nif' --output 'local\nif-index.json'
```

Output parents must already exist. Reports are published without replacing earlier
reports, and destinations inside the selected installation are refused. Exit 2
means input findings are present; exit 1 means a command failed. Exit 0 establishes
only the documented census scope, never gameplay parity.

## SE and AE

The exact executable version, executable hash, plugin/header versions, archive
versions and installed optional content are separate facts. A Special Edition
purchase can install an updated runtime. Both HEDR 1.70 and 1.71 are supported for
metadata; 1.71's expanded ESL range is checked without truncating full record IDs.
Unknown header versions produce findings instead of borrowing an assumed dialect.
Native SKSE DLL support is a separate future capability, not supplied here.
The measured Steam installation reports runtime 1.7.104.0/build 24914197 and has
the five base masters, a resource-pack ESL, and four included Creation plugins.
This is not the full paid Anniversary content set. ESL-owned record IDs stay
file-local; references to other light plugins and winning overrides still require
a verified profile/load-order layer. Light-plugin status comes from the TES4 header
flag; an `.esl` suffix is recorded separately.

An independent offline metadata check reuses the NV project's pinned esplugin
reference with its SkyrimSE dialect. It is a separate tool, outside the runtime:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --manifest-path tools/plugin-oracle/Cargo.toml --jobs 1
.\target\debug\skyrim-plugin-oracle.exe local\census-new.json
```

This compares physical record counts, master lists, header versions and light-plugin
status. Separate pinned Mutagen tools compare ordered VMAD fields and archived
script payload bytes. They use the installed .NET SDK with private package and
build directories; they are not part of the Rust runtime:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/dotnet.ps1 restore tools/vmad-oracle/vmad-oracle.csproj --locked-mode --disable-parallel
powershell -NoProfile -ExecutionPolicy Bypass -Command "& './tools/dotnet.ps1' build tools/vmad-oracle/vmad-oracle.csproj --no-restore --disable-build-servers '-m:1'"
powershell -NoProfile -ExecutionPolicy Bypass -File tools/dotnet.ps1 restore tools/archive-oracle/archive-oracle.csproj --locked-mode --disable-parallel
powershell -NoProfile -ExecutionPolicy Bypass -Command "& './tools/dotnet.ps1' build tools/archive-oracle/archive-oracle.csproj --no-restore --disable-build-servers '-m:1'"
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-bindings.ps1 -Census local/census-new.json
.\local\archive-oracle\bin\Debug\net9.0\archive-oracle.exe local/census-new.json
```

Finish Rust builds before launching a census/export; Windows locks a running
executable. `verify-bindings.ps1` creates a new evidence directory and hash receipt.
The current strict VMAD comparison exits 2 for the documented Dawnguard source-ID
normalization difference. It must not be reported as an all-pass result. The
archive check covers script payloads; meshes, textures, audio and other payloads
still need independent format and behavior checks.

Mutation controls are reproducible with `tools/test-vmad-oracle.ps1 -Plugin <file>
-Export <its.jsonl>` and `tools/test-archive-oracle.ps1 -Census <census.json>`.
They change only ignored evidence copies, require an unaltered passing control,
and verify that a changed field/hash fails even when counts stay the same.

See [integration boundaries](docs/shared-integration.md), [source evidence](sources.lock.json),
[binding contract](docs/binding-contract.md), [remaining work](NEXT_STEPS.md) and
[requirements](parity/requirements.json).
