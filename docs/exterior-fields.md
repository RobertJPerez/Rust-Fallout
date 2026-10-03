# Exterior source inspection

The terrain command reads a real winning exterior CELL, follows its worldspace
group and parent-world chain, and decodes winning LAND records assigned to that
cell. It uses the existing record store, strict on-demand decompression and optional
source-bound metadata cache. Source files remain read-only.

This is the next content-loading layer for Goodsprings. It does not draw terrain
or implement traversal. [Checkpoint 10](terrain-heights.md) adds optional height
reconstruction under a pinned reference model; [checkpoint 11](terrain-geometry.md)
adds source geometry and an unlit preview. Retail normal interpretation, measured
engine units, parent inheritance and water behavior remain open.

## Run it

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli
New-Item -ItemType Directory -Force .\local\record-index-cache | Out-Null
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --index-cache .\local\record-index-cache --output .\local\terrain-goodsprings-new.json
~~~

Use a new report filename each time. Omitting --index-cache uses the uncached
metadata path. Every selected body remains strict on both paths. The known unrelated
LAND checksum defect stays deferred; selecting a corrupt LAND fails rather than
enabling checksum recovery.

The report retains source-plugin hashes, record headers, canonical identities,
decoded-body hashes and field offsets within decoded bodies. World and landscape
links are checked against WRLD, CLMT, WATR, IMGS, ECZN, MUSC and LTEX target kinds.
Null, missing, deleted and wrong-kind targets remain distinguishable. Missing or
wrong-kind parent worlds block chain traversal. Non-parent link failures produce a
report and exit code 1. Runtime readiness remains false.

## Fields and boundaries

The format reference is [xEdit at 9fb0168](https://github.com/TES5Edit/TES5Edit/tree/9fb016884bec138ea6c7b872cec831537d464c3e).
Core/wbDefinitionsFNV.pas defines WRLD and LAND; Core/wbDefinitionsCommon.pas defines
the shared landscape fields and VTXT position validation. Both carry MPL-2.0
notices. These definitions were inspected as references; no Pascal implementation
was copied or linked, and xEdit itself has not been run.

| Record | Selected fields |
| --- | --- |
| CELL | EDID, FULL, DATA; signed XCLC coordinates and optional raw trailing word (one land flag byte plus three unused bytes) |
| WRLD | EDID, FULL, DATA; WNAM/PNAM parent and flags; climate, water, LOD water, image space, encounter zone and music links; NAM4/DNAM float bits |
| LAND | DATA; 33 by 33 VNML/VCLR triplets; VHGT offset bits, signed deltas and three padding bytes; ordered BTXT/ATXT headers and associated VTXT entries |

All other fields retain their signature, decoded offset and exact bytes. Known
singletons reject duplicates and wrong lengths. Strings require one final NUL;
float fields must be finite. Float bit patterns preserve signed zero. Padding is
retained rather than required to be zero. Texture quadrants are 0–3; VTXT positions
are 0–288 within the schema's 17 by 17 quadrant. Invalid alpha framing fails.
Absent fields stay absent; editor defaults are not silently manufactured.

Decoder input is capped at 64 MiB per body. Before allocating field vectors, a
framing pass checks a conservative 128 MiB retained-field storage estimate, including
payload elements and per-field metadata. This is not a measured peak-memory limit;
allocator overhead and surrounding record-store storage are outside that estimate.
Parent-world traversal is iterative, detects cycles and caps chains at 256 records.
These are inspection budgets, not assertions about authored gameplay limits.

Winning parent membership uses the defining plugin's master table. A moved LAND
override belongs to its winning cell. Deleted winners remain explicit tombstones,
and an override replaces its complete body; earlier subfields are not merged in.
LAND world-group membership must agree with the selected CELL. Parent worlds retain
separate fields; inheritance flags are reported without applying unmeasured rules.

## Independent field comparison

The separate tools/terrain-oracle C++ executable projects the selected schema fields
from tagged decoded bodies. It uses only the C++ standard library and Windows CNG
for SHA-256. It is original project tooling, has no third-party parser dependency,
and is never linked into the Rust application. Its version is bound by the source
snapshot and actual executable hash.

This oracle is a separately authored field reader. It is not an xEdit executable
or a retail behavior oracle. It starts after Rust's strict decompression, so its
comparison does not independently validate decompression or override resolution.
Those remain separate parser/resolver test and corpus scopes.

Build the helper with the installed Visual Studio CMake:

~~~powershell
$vswhere = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio\Installer\vswhere.exe'
$terrainVs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$terrainCmake = Join-Path $terrainVs 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
& $terrainCmake -S tools/terrain-oracle -B local/terrain-oracle-build -A x64
& $terrainCmake --build local/terrain-oracle-build --config Release
~~~

For a manual comparison, choose a dedicated existing cache outside the installation:

~~~powershell
New-Item -ItemType Directory -Force .\local\terrain-bodies-new | Out-Null
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --body-cache .\local\terrain-bodies-new --output .\local\terrain-fields-new.json
.\local\terrain-oracle-build\Release\terrain-oracle.exe .\local\terrain-bodies-new | Set-Content -Encoding UTF8 .\local\terrain-oracle-new.json
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --body-cache .\local\terrain-bodies-new --oracle-report .\local\terrain-oracle-new.json --output .\local\terrain-compared-new.json
~~~

Body-cache blobs contain a four-byte record-kind tag followed by exact strictly
decoded source bytes. Publication/reuse uses the existing verified cache and commit
marker. These are local retail-derived inputs; they are not checked into Git.
The comparer requires exactly the selected body set, rejects changed digests and
reports exact differing field paths. It caps diagnostics at sixteen paths per body.

## Checkpoint evidence

The preflight Goodsprings CELL has grid coordinates [-18, 0], one LAND with 27 layers
and a winning WastelandNV definition from LonesomeRoad.esm. GoodspringsSource is at
[-16, -5]. The declared order, rather than the base plugin alone, selects these
definitions. Source group labels and offsets remain attached to every result.

The Rust evidence runner compares Goodsprings, GoodspringsSource, TownCenter,
NVDLC02PineCreek, NVDLC03SLVillage and NVDLC04DivideEast. These cover base LAND plus
one exterior in each of the four campaign DLCs. The last fixture follows two parent
worlds. GRA and the four preorder packs remain in the supplied ten-plugin order;
they are not claimed to contribute new exterior samples.

~~~powershell
# Commit the implementation and choose a fresh run directory.
.\target\release\fallout-evidence.exe --checkpoint 9 --run-directory .\local\terrain-09-verified --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
~~~

The runner checks cached/uncached source-field equality, independent field equality,
source/body/executable hashes and a deliberately altered height-offset bit that must
produce exit code 1. It also runs workspace checks and rehashes the original installation.
Raw bodies and field reports stay local. See [exterior-fields.json](../reports/exterior-fields.json)
for counts, source identities, hashes and the declared comparison scope.

Sixteen original integration tests cover raw bits/padding, schema bounds, duplicate
fields, alpha framing, moved/deleted winners, master-relative rebinding, parent chains,
wrong-kind links, strict checksum failures, cache/uncached equivalence, source hashes,
protected output and deterministic mutation checks. They do not establish terrain,
water, physics or campaign behavior.
