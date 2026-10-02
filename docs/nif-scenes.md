# NIF scene and mesh decoding

The production Rust reader's scene/mesh layer decodes six payload types: `NiNode`, `BSFadeNode`,
`NiTriShape`, `NiTriStrips`, `NiTriShapeData`, and `NiTriStripsData`. Each supported
block must consume exactly its declared byte range. Names, controller/extra-data
links, flags, property/collision links, local transforms, geometry/skin links, and
geometry material metadata are retained. The additional material/texture payloads
are documented in [material decoding](nif-materials.md); skin and collision payloads
remain unimplemented.

Meshes preserve positions, normals, tangents, bitangents, colors, UVs, bounds,
consistency flags, raw topology and match groups. Missing arrays stay missing. The
decoder does not fabricate normals, repair indices, normalize matrices or replace
nonfinite values. Unknown blocks remain addressable through the container index.

Strip winding alternates on every source step, including degenerate connector
steps. Repeated-index connector triangles are omitted from the expanded triangle
list; parity resets at each new strip. The stored triangle count includes those
steps. In the house fixture, 91,760 connector steps accompany 84,403 output triangles.
This distinction is covered by an authored fixture and raw oracle comparison.

The graph resolver validates supported child links and exact geometry-data types,
rejects cycles and repeated/multiple parents, and composes parent-first affine
transforms without recursion. It visits disconnected components and marks footer
reachability separately. Unknown scene targets are listed, including the ambient
light in `lamplightchandelier.nif`. Property/controller/skin/collision links are
range-checked, but their complete type and behavior contracts remain future work.

Source matrices are retained as stored triples. Their row interpretation is checked
against nifly's transform API, including composed transforms. The nifxml description
uses column-major terminology; renderer integration must verify the complete axis,
handedness, unit and placement-rotation convention with visual fixtures instead of
assuming a label settles it. Composed matrices use f64; source fields remain f32.

## Evidence

The 207 distinct Doc Mitchell models contain 744 supported objects, 401 meshes,
96,437 vertices and 84,403 expanded triangles. All decoded object fields and stored
attributes match the independent oracle, with identical source f32 bits and exact
reference/topology arrays. Composed transforms compare with absolute tolerance
`1e-4` or relative tolerance `1e-5`; the largest absolute difference in this fixture
is approximately `1.353e-5` source units. The eleven older-stream samples add seven
objects, two meshes, 270 vertices and 258 triangles. Several are animation-only;
this sample does not prove every payload schema in every stream revision.

The full 21-archive scan visits 25,760 NIF/KF members. Six fail the existing legacy
container gate. Of the remaining 25,754, supported payloads decode in 25,729 files;
25 fail finite-number or vertex-index validation. Successful files contain 133,398
supported objects and 67,798 geometry-data blocks, representing 38,958,598 vertices
and 43,335,965 expanded triangles. These totals include data not attached to a
supported visible object, archive overrides, animation files and unsupported
branches. They are inspection totals, not rendered-content counts.

Full payload scanning is not full independent comparison. Exact independent
comparison covers 218 files and 403 meshes; it does not cover the entire corpus.
The original four negative comparison checks alter one float bit, reverse triangle winding,
change a composed translation, and change the expected source digest. Each fails,
while the unmodified input passes. Original files are never changed for these checks.
The material extension adds checks for changed alpha/path fields and missing projections.

The raw diagnostic oracle independently confirms issues in all 25 rejected files:
2,235 nonfinite color components, 284 nonfinite UV components, six nonfinite
translation components, and eleven out-of-range match-group indices. The first
21 files are rejected for nonfinite values and four for match-group indices.
These are source-data observations, not evidence for a particular retail repair.
No global sanitization or compatibility exception was added. See the file hashes,
paths and field categories in [the report](../reports/nif-scenes.json).

## Reproduce

First create the cached cell models using [world inspection](world-inspection.md).
Use new output paths because inspection reports never overwrite existing evidence.

```powershell
py -3 tools\probe-nif-scenes.py --cell local\cell-docmitchell-final.json --cache local\docmitchell-models --output-dir local\scenes-new
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-nif-oracle.ps1
py -3 tools\run-nif-oracle.py --input local\docmitchell-models --mode scene --output local\oracle-scenes-new.json
py -3 tools\compare-nif-scenes.py --scenes local\scenes-new --oracle local\oracle-scenes-new.json --cache local\docmitchell-models --output local\comparison-scenes-new.json
py -3 tools\check-nif-scene-comparison.py --scenes local\scenes-new --oracle local\oracle-scenes-new.json --cache local\docmitchell-models --output local\negative-scenes-new.json
.\target\release\fallout.exe nif-census --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --inspect-scenes --output local\scene-census-new.json
```

The capture wrapper writes stdout bytes directly, avoiding Windows PowerShell 5's
UTF-16 redirect default. Outputs contain retail geometry and stay under ignored
`local/`; public summaries contain metadata only.

The GPL nifly oracle remains a separate executable. `--scene` loads raw blocks through
its factories and checks consumed sizes before reading their fields; it bypasses
`NifFile::PrepareData`, which can remove invalid triangles and clean texture paths.
Its math API supplies the independent transform composition and strip conversion.
`--scene-diagnostics` instead reports nonfinite attribute bits and invalid index
locations. The Rust runtime links none of this C++ code, and no upstream schema
generator was used for the Rust decoder.

The separate [model preview](model-preview.md) now renders supported geometry with
unlit diffuse textures. Controller evaluation, skinning, Bethesda shaders, collision
and gameplay execution remain absent. The source decoder retains visibility flags;
the preview evaluates only its documented subset.
Null mesh data, missing arrays and unsupported branches remain distinguishable;
every scene reports `runtime_ready=false`.
