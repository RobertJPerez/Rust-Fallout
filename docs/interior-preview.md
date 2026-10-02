# Placed interior inspection

`fallout-preview` now assembles `GSDocMitchellHouse` from the actual winning CELL and
placed references, archived models and diffuse textures. It keeps the installation
read-only. The two reviewed GPU views show the cabinet room and bedroom. The reports
retain omitted references and material warnings; this is still an inspection host.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked -p fallout-preview
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --cell GSDocMitchellHouse --load-order profiles/nv-inspection-order.json --camera-position 2130 2130 7440 --camera-look-at 1883 1763 7420
```

Camera coordinates use the original source units and axes. Explicit camera arguments
start in fly mode. WASD moves, Q/E moves vertically, arrows look, Shift moves faster,
R resets and Escape closes. Tab switches orbit/fly. Omitting camera arguments starts
with an orbit of the complete prepared bounds. Movement has no collision and has not
received independent keyboard automation coverage.

The load order is explicit fixture input, not the original game's measured effective
profile. The view currently requires an interior CELL and unambiguous archive paths.
It does not guess loose/archive precedence or activate anything in the original game.

## Prepared content

The fixture contains 435 placed references and 222 distinct bases. The view draws
400 references as 722 mesh instances using 203 shared models. It uploads each mesh
and material once, then creates presentation entities retaining each reference's
FormKey. Textures share storage by normalized path and sampler: 146 pairs in this
fixture. Prepared geometry contains 96,394 vertices and 84,378 triangles before
placement instancing. Full source geometry remains unchanged in the decoder.

Three initially disabled references, one actor and 31 references without a supported
generic MODL are omitted and individually reported. Some latter cases need alternate
item-model selection; they do not imply missing game files. Enable-parent evaluation,
actors, skinning, controllers, effects and gameplay remain separate work.

The view consumes the strict deferred record store. It validates TES4/CELL metadata,
indexes all headers/identities/parent ranges and strictly decodes selected bodies on
access. The 585,196 bodies deferred at indexing time are explicitly unvalidated.
No LAND checksum exception was added. All ten selected source-field groups match the
earlier full diagnostic report, including identities, links, transforms and archive
candidates. See [world inspection](world-inspection.md).

## Placement and rendering limits

Static reference math uses clockwise X, then Y, then Z: `Rz(-z) Ry(-y) Rx(-x)`.
This convention was researched in pinned
[OpenMW conversion](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/misc/convert.hpp)
and [static scene placement](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/apps/openmw/mwworld/scene.cpp).
It is provisional, not measured NV retail parity. Actor rotations require their own
rules. Original transform fields stay intact; the engine-independent adapter composes
in f64, subtracts a stable source origin, then presentation converts `[x,y,z]` to
`[x,z,-y]` and narrows the relative transform to f32. Analytic tests cover rotation
order, scale, translation and sub-unit rebasing at large coordinates.

BSXFlags metadata now preserves the source name and all value bits. Its 175 blocks
in the 218 model samples agree exactly with the independent raw nifly reader. The
presentation omits named EditorMarker/VisibilityEditorMarker meshes when their root
declares BSX bit 5, following the pinned
[marker selection reference](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/nifosg/nifloader.cpp).
Two such submeshes are omitted in the house. Standalone audio/heading marker models
remain visible; no filename filter silently removes them.

Unlit diffuse materials remain an approximation. The bedroom window contains visible
magenta fallback surfaces, and transparent effects do not reproduce retail shading.
The real geometry/textures and successful captures establish a working assembly/GPU
path, not a visually complete interior. Collision, source camera measurements, retail
lighting/material comparisons and player simulation are still acceptance gates.

Preparation is synchronous and has explicit cell/model/geometry/texture budgets.
Debug capture runs took approximately 42–43 seconds including loading and rendering
on this machine. These are smoke-run durations, not a startup or frame-rate benchmark.
Disk indexes, cancellable preparation, streaming and measured performance are open.

## Reproduce the captures and comparison

```powershell
py -3 tools/smoke-interior.py --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --output-dir local/interior-new
.\target\release\fallout.exe cell --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id GSDocMitchellHouse --defer-unread-payloads --output local/cell-strict-new.json
py -3 tools/compare-cell-selection.py --full local/cell-docmitchell-final.json --deferred local/cell-strict-new.json --output local/cell-selection-new.json
```

Use fresh output paths. The capture manifest binds both PNG/JSON pairs to the preview
binary, load-order digest and source camera coordinates. The report retains hashes of
all selected plugins and asset bytes. The comparison requires the earlier local full
fixture, which is not distributed. Raw screenshots, models and textures stay under
ignored `local/`; only code and evidence metadata are published. See
[interior-preview.json](../reports/interior-preview.json) for counts, hashes and limits.
