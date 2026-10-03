# Textured terrain inspection and first manual test

The viewer can now draw authored diffuse terrain textures with the quadrant weights
from checkpoint 13. This is an inspection build: retail blend rules, tiling,
lighting, default materials, water, props, collision, streaming and gameplay are
unfinished. A terrain-only camera view does not establish a playable New Vegas.

## Run the build

From `G:\Rust-Fallout`, run:

```powershell
.\target\debug\fallout-playtest.exe
```

You can also double-click that executable in Explorer. The ignored
`local/playtest.json` is already configured for this installation and Goodsprings.
The 3D view opens in a separate window. The console now shows loading stages and
an elapsed-time message every five seconds until data preparation finishes. The
latest Goodsprings offscreen launch prepared source data in 17 seconds; this is a
single smoke run, not a startup performance guarantee. Keep the launcher console
open while loading.
`fallout-playtest.exe --terrain GoodspringsSource` selects another supported cell;
`--check` validates the configuration without opening a window.
`--smoke-test` exercises the complete launcher and renderer offscreen, writes a
capture/report, and exits. Each launch saves viewer output to its own
`local/playtest-*/startup.log`. Failures keep an interactive console open until
Enter is pressed so the error is readable. The launcher stays
in Rust and never starts the retail game or changes source data.

For another checkout, create `local/playtest.json` with schema_version 1, install
as the absolute NV directory, load_order as profiles/nv-inspection-order.json,
terrain as Goodsprings, and texture_repeats_per_quadrant as 4. The launcher
validates bounded JSON and required paths before starting the viewer.

The equivalent direct viewer command is:

```powershell
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --terrain Goodsprings --terrain-textures --terrain-texture-repeat 4
```

The executable was built and tested here. To rebuild it:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked -p fallout-preview
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked -p fallout-cli --bin fallout-playtest
```

The repetition value is an explicit preview setting, measured per quadrant. Four
repeats is the automated fixture's setting, not an established retail scale.
GoodspringsSource and NVDLC02PineCreek also have fully authored nonnull material
chains. Substitute either editor ID in the command above. TownCenter lacks base
layers, and NVDLC03SLVillage/NVDLC04DivideEast contain unapplied NULL default layers.
Textured mode refuses these cases. Their existing vertex-color view still runs
when both texture options are omitted.

## What needs your test

1. Open Goodsprings and check that the terrain and textures appear. Orbit with A/D
   or left/right arrows, tilt with W/S, and zoom with Q/E. R resets the view.
2. Press Tab for free flight. WASD moves, Q/E moves down/up, arrows turn, and Shift
   moves faster. Confirm directions, focus, reset behavior and clean exit with Esc.
   Movement is an inspection camera; it has no player collision or actor rules.
3. Inspect transitions at the two quadrant boundaries. Record visible cracks,
   orientation changes, unexpected black areas or layer seams. Excess overlay
   coverage is preserved rather than normalized, and may brighten the image.
4. Compare the same locations in your original New Vegas installation. Identify
   texture orientation, repetition and terrain/layer differences. Keep your game
   configuration and mod/load order fixed, and preserve your existing saves.
   These observations guide experiments; unmatched cameras are not parity evidence.

For a repeatable inspection capture, use fresh output names:

```powershell
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --terrain Goodsprings --terrain-textures --terrain-texture-repeat 4 --headless --capture local/goodsprings-manual-new.png --report local/goodsprings-manual-new.json
```

The report records original source identities, archive/texture hashes, weights,
camera origin, diffuse upload metadata and draw counts. Source-camera arguments
remain available as documented in [terrain geometry](terrain-geometry.md).

## Implementation and automated evidence

Textures share the existing safe archive lookup and bounded BC1/BC2/BC3 DDS upload.
Each quadrant has an opaque residual-base pass followed by depth-tested additive
overlay passes. Linear RGB receives authored vertex colors and interpolated byte
weights; overlay passes leave depth unchanged. This deliberately explicit adapter
does not reconstruct normal/specular shaders or measured retail compositing.
Source grids, bodies and images remain immutable. The byte-weight model reports
missing alpha, excess coverage and unapplied defaults instead of rewriting them.

The geometry remains 1,089 source vertices and 2,048 triangles per complete cell.
Layer passes duplicate quadrant vertices/indices for drawing; reports distinguish
source counts from uploaded/drawn counts. Shared diffuse handles avoid duplicate
image uploads within a cell. This bounded first renderer favors traceability;
single-pass batching remains a future measured optimization.

Checkpoint 14 compares all six source fixtures again, including 31,212 weights and
40 unique archive members. GPU evidence covers three textured terrain cells, the
TownCenter vertex-color regression, Doc Mitchell's interior, and 64 original
numeric material checks. Two checks verify textured residual-base/additive passes.
Three incomplete material cases must fail before any capture is created.
Captures remain local; only counts, hashes and synthetic expectations are public.

The Windows computer-use helper could not enumerate apps: its native pipe was
missing on the initial call, retry and reset/reinitialization. Headless Rust GPU
tests remain available, but retail UI/camera comparison requires a direct manual
test in this environment. No retail image comparison or accepted scenario is claimed.

```powershell
# Build/commit first, then use a fresh evidence directory. Keep published receipts unchanged.
.\target\release\fallout-evidence.exe --checkpoint 14 --run-directory .\local\terrain-14-new --no-publish --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
```

See [textured GPU evidence](../reports/terrain-textured-preview.json). Earlier
immutable checkpoints and the checkpoint 11 untextured captures remain retained.

The runner publishes checkpoint-specific files and checks every destination before
writing. Current aliases are updated separately after successful verification.
`--no-publish` repeats all checks and saves its metadata beneath the new local run
directory; omit it only when publishing a checkpoint that has no existing receipts.
Verification records completion as UTC Unix seconds, rather than a fixed date.
