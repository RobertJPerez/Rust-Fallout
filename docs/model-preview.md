# Model inspection with Bevy

`fallout-preview` renders an actual archived NIF through the production Rust decoder
and unique archive lookup. It uses Bevy 0.19.1 with DDS support. It does not require
exporting or converting the installation, and writes no files into it.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\cargo.ps1 build --locked -p fallout-preview
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --model meshes/architecture/goodsprings/nv_vitomaticvigortester_cabinet02.nif
```

Use A/D or the left/right arrows to orbit, W/S to tilt, Q/E to zoom, R to reset,
and Escape to close. The camera frames the model bounds in source units. This is an
inspection camera; player movement and collision are not implemented.

For a reproducible GPU capture without a desktop window:

```powershell
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --model meshes/furniture/chair01.nif --headless --capture local\chair-new.png --report local\chair-new.json
```

Capture mode exits after PNG readback and writing complete. It rejects existing
output files, requires existing parent directories outside the installation, and
has a capture timeout. The JSON records source hashes, geometry counts, texture
formats/mip counts, bounds, and unsupported features. It describes prepared content;
the successful PNG and process exit are separate evidence of GPU rendering.

## What this view does

The presentation adapter rotates `[x,y,z]` to `[x,z,-y]`, retaining source distances
and UVs. It bakes composed source transforms into positions, uses inverse-transpose
normal transforms, and reverses winding for a reflected transform. No gameplay unit
conversion or plugin placement rotation is established by this model-only view.

Only reachable, visible, unskinned meshes with supported geometry are drawn. Hidden
flags on ancestors are honored. Unsupported blocks, skinned meshes and property
inheritance are reported. Missing normals can be generated for presentation; the
source decoder never changes their presence or values.

Bevy loads BC1/BC2/BC3 DDS pixels directly; the adapter checks dimensions, mip count
and payload length before upload. Diffuse images use sRGB, source UVs and the shader
clamp mode. The view uses unlit diffuse color and vertex colors, with approximate
alpha blend/test modes. Unresolved material selection appears magenta and is named
in the report. It does not evaluate normal/specular/environment maps, shader flags,
retail lighting, animation, collision or gameplay.

The chair and Vit-o-matic captures establish that the complete loading and GPU path
works for those fixtures. They are not image comparisons against the original game.
The cabinet has an unsupported diffuse binding explicitly reported by the preview.
Retail visual parity and Goodsprings remain later gates.

Checkpoint 05 adds a separate [placed interior preview](interior-preview.md), shared
texture storage and an optional fly camera. The model mode still uses the same
decoder and diffuse adapter. Two fresh model captures passed after these changes;
neither the model nor interior view establishes retail shader or collision parity.

Retail-derived PNGs and per-model reports stay under ignored `local/`. They are not
bundled with the source or treated as redistributable art.
