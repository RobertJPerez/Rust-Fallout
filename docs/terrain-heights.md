# Exterior height reconstruction

The optional height stage converts a winning LAND's VHGT into 1,089 binary32
samples. Its original offset, signed deltas, padding and unknown fields remain
separate, with source offsets and hashes. Missing VHGT and deleted LAND stay
explicit; neither receives a default surface.

The model is `esm4-vhgt-f32-row-prefix-scale8-v1`. It follows the convention in
[pinned OpenMW height loading](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/esm/esmterrain.cpp)
and [LAND dimensions](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/esm4/loadland.hpp).
This is reference-model evidence, not measured retail NV behavior. New Rust and
offline C++ code are original project implementations; no OpenMW implementation
was copied or linked, and OpenMW has not been built here. The source lock records
its GPL-3.0 root license and the LAND files' permissive per-file notices.

## Integration and boundaries

Samples use index `y * 33 + x`. Each row's first delta adds to the previous row's
first accumulator. Horizontal deltas then accumulate within that row. The initial
delta also applies to the stored offset. Each addition rounds in f32 before the
output is multiplied by eight. A single prefix sum over the whole array carries
the wrong row value; integer summation or f64 accumulation also changes rounding.

The grid contains exactly 33 by 33 finite samples, with private storage and bounded
queries. Invalid counts, non-finite offsets and scaled overflow fail. Position
queries use the reference's 4,096-unit cell width and 128-unit spacing in f64,
including negative and extreme signed coordinates. A renderer must rebase before
casting to f32. Measured axes/units, interpolation and collision remain open.

~~~powershell
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --reconstruct-heights --output .\local\goodsprings-heights-new.json
~~~

The added surface reports canonical cell/world/LAND identities, source body hashes,
height-field offsets, status and exact height bits. Conversion is opt-in; ordinary
source inspection does not apply it or reject a finite offset solely because its
scaled height would overflow.

Many exterior cells have no EDID. Select a neighbor by its origin plugin and local
ID, without transient load-order bits:

~~~powershell
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --reconstruct-heights --neighbor-form FalloutNV.esm:DAEB9 --output .\local\goodsprings-east-edge-new.json
~~~

Use --neighbor-editor-id when the neighbor has a name. Different worlds,
nonadjacent/diagonal cells, ambiguous LAND sets and missing heights fail. Diagnostics
report every differing edge sample and the maximum numeric difference without
stitching, averaging or replacing either surface. A difference is an observation,
not proof of a terrain defect or an engine rule. Parent inheritance is unimplemented.

## Comparison and reproduction

Build the offline oracle as described in [exterior fields](exterior-fields.md).
Populate a dedicated body cache with the terrain command's --body-cache option,
then compare the same selected set:

~~~powershell
.\local\terrain-oracle-build\Release\terrain-oracle.exe .\local\terrain-bodies-new --heights | Set-Content -Encoding UTF8 .\local\terrain-heights-oracle-new.json
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --reconstruct-heights --body-cache .\local\terrain-bodies-new --oracle-report .\local\terrain-heights-oracle-new.json --output .\local\terrain-heights-compared-new.json
~~~

The runtime uses a linear pass. The authored C++ oracle evaluates each vertex
separately along its vertical and horizontal dependency paths, with MSVC /fp:strict.
The comparer checks raw fields, body hashes, every derived sample, extrema and model
identity. Missing height projection fails; a changed derived bit produces exit 1.
This is neither an executed OpenMW/xEdit application nor a retail behavior oracle.
Independent decompression and canonical resolution remain outside its scope.

~~~powershell
# Commit the implementation, then choose a fresh directory.
.\target\release\fallout-evidence.exe --checkpoint 10 --run-directory .\local\terrain-10-verified --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
~~~

The runner repeats the six base/DLC fixtures with cached and uncached reads, compares
36 original gradient fixtures and a native overflow negative case, and diagnoses
four Goodsprings boundaries. Neighbor fields retain strict provenance but are not
separately compared by the native oracle. Workspace checks and a fresh full baseline
bind the source and executables. Raw records/bodies remain local; public metadata is
in [terrain-heights.json](../reports/terrain-heights.json).

Original tests cover analytic surfaces, binary32 rounding, count/numeric bounds,
four edge directions, large signed coordinates, absent/deleted LAND, unnamed cells,
world mismatches and ambiguous surfaces. Terrain meshes, normals, materials, water,
physics, streaming and measured retail comparisons remain open. No rendered terrain
or gameplay scenario is accepted by this checkpoint.
