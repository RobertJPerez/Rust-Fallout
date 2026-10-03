# Authored terrain blend maps

Rust expands each quadrant's sparse ATXT/VTXT samples into 17-by-17 inspection
weights. BTXT retains its source identity and receives the saturating remainder
after overlay coverage. Every output layer points back to its original layer index;
source float bits, offsets and unknown bytes remain unchanged.

```powershell
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --inspect-blends --output .\local\goodsprings-blends-new.json
```

The explicit model is `esm4-local-u8-residual-base-v1`. The pinned
[OpenMW ESM4 loader](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/esm4/loadland.cpp)
informs contiguous, source-ordered alpha indices; the
[terrain storage reference](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/esmterrain/storage.cpp)
informs byte quantization and residual base coverage. Our original Rust/C++
calculations use quadrant-local grids, including boundaries. They do not reproduce
reference chunk sampling, neighbor repair, merged maps or render implementation.

Opacity is multiplied by 255 in binary32 and truncated to a byte. Finite values
outside zero through one clamp explicitly; the report counts them. Source values
remain unchanged. Overlays are not renormalized when their sum exceeds 255; excess
coverage is counted, and base coverage saturates at zero. Missing alpha data and an
authored empty alpha array remain distinct, both expanding to zero inspection
coverage. Duplicate positions/bases, ambiguous order and nonfinite values reject
interpretation. At most 256 layers expand, bounding sample storage to 256 times
289 bytes plus metadata.

NULL textures and absent bases remain explicit. TownCenter has an overlay without
any base layer, and the selected DLC fixtures contain four NULL layers. These
source weights can expand without inventing a complete material or a default.

The independent C++ oracle rereads tagged decoded bodies and evaluates coverage
per vertex. Run it with `--geometry --blends`, then use `--oracle-report`,
`--body-cache` and `--inspect-blends` in the Rust inspector. Exact byte comparison
reports changed positions. Compression, linking and retail rendering are outside
that oracle.

```powershell
# Build and commit first; use a fresh evidence directory.
.\target\release\fallout-evidence.exe --checkpoint 13 --run-directory .\local\terrain-13-verified --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
```

Six base/DLC cells compare selected fields, geometry, weights, cached/uncached
reports and texture-member bytes. A changed oracle weight and damaged copied cache
must fail. Workspace checks and full original-installation hashing also run. Public
summaries contain counts/hashes; source bodies, weight arrays and image payloads
stay local. No new retail rendering, gameplay, default-layer or M1 acceptance is claimed.
