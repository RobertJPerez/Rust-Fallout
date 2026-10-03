# Source terrain geometry and preview

The terrain inspector can now build a 33-by-33 source surface. The separate Bevy
adapter displays that real winning LAND without replacing it with generated terrain.
Height fields, authored normals/colors, unknown bytes and source receipts stay intact.
This is an inspection step toward Goodsprings; M1 and gameplay acceptance remain open.

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --reconstruct-heights --inspect-mesh --output .\local\goodsprings-mesh-new.json

powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked -p fallout-preview
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --terrain Goodsprings --load-order profiles/nv-inspection-order.json
~~~

The existing orbit/fly controls and optional paired source-camera coordinates work
in this mode. The camera passes through terrain. Add --headless, --capture and
--report with fresh paths outside the installation to save a GPU inspection capture.

## Geometry contract

The model `esm4-source-grid-checkerboard-positive-z-v1` uses x-fast indexing,
128-unit vertex spacing and the checkpoint 10 height model. The CPU mesh retains
f64 local positions, normalized f32 normal bits, optional raw color triplets, indices
and bounds. Arrays are bounded by 1,089 vertices and 6,144 indices. Heights are required;
invalid counts, zero authored normals, nonfinite conversion and unknown hide bits fail.
Absent normals/colors remain absent in the CPU surface.

[Pinned OpenMW buffer generation](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/terrain/buffercache.cpp)
informs alternating diagonals. Our inspection triangles face positive source Z;
this winding is not a measured retail NV rule. Signed VNML bytes are normalized
in f64 and narrowed once. We retain authored boundary/corner normals; OpenMW's
neighbor substitution and corner averaging are not implemented. Neither OpenMW
nor xEdit was built or run here, and no upstream implementation was copied or linked.

XCLC's trailing four bytes contain one land flag byte and three unused bytes in
[the pinned xEdit shared definition](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsCommon.pas).
The existing quadrant_flags report preserves that entire raw word. Only its low
byte enters the hide mask. The four low bits hide 16-by-16 quad regions using
quadrant = (x >= 16) + 2 * (y >= 16). All 16 masks have coverage/orientation tests;
the six real native-comparison fixtures have no hidden quadrants. This is not a
retail hide-mask behavior comparison. An absent flag field stays absent in reports;
inspection draws the complete grid in that case.

The preview requires one present winning LAND with authored normals. It subtracts
the cell-centered origin in f64 before rotating [x,y,z] to [x,z,-y] and narrowing to
relative f32. The rotation preserves triangle orientation. Unlit VCLR bytes divided
by 255 provide inspection colors; their retail color-space interpretation is open.
TownCenter has no VCLR and uses a labeled white inspection material. No source
color is invented. Zero UV coordinates only satisfy the adapter's vertex interface;
no landscape texture is sampled.

## Evidence and reproduction

Build the C++ helper using [the exterior-field instructions](exterior-fields.md).
It accepts --geometry, which also includes reconstructed heights. Compare its output
with --reconstruct-heights --inspect-mesh --body-cache and --oracle-report on the CLI.
The original C++ projection reads the same tagged strict decoded bodies, independently
evaluates heights, and compares positions, normal bits, colors, indices and bounds
exactly. It does not independently certify compression, override resolution, CELL
hide semantics or retail rendering. Reversing one triangle's winding must fail.

~~~powershell
# Commit the implementation before binding its evidence; use a fresh directory.
.\target\release\fallout-evidence.exe --checkpoint 11 --run-directory .\local\terrain-11-verified --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
~~~

The runner compares six base/DLC surfaces with cached and uncached reads, renders
five terrain cells, captures an interior regression, runs workspace checks and hashes
the full original installation. GPU smoke checks require expected geometry counts,
a 1280-by-900 PNG and nonempty pixel occupancy. They are separate from visual review
and do not certify retail appearance. Binary identities include the debug preview,
release CLI, evidence runner and native oracle. Raw bodies and captures stay ignored
under local; public reports contain counts, identities and hashes.

[terrain-geometry.json](../reports/terrain-geometry.json) and
[terrain-preview.json](../reports/terrain-preview.json) record those scopes. Checkpoint
10's original gradients/boundary diagnostics and checkpoint 06's material GPU oracle
checks retain their earlier evidence; checkpoint 11 does not silently recertify them.

Landscape textures/blending, parent inheritance, normal repair, props, water, retail
lighting, streaming, collision and gameplay are unfinished. Effective original
profiles, archive precedence and vanilla format exceptions still block M1.
