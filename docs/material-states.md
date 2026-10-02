# Source material states in the inspection viewer

Checkpoint 06 fixes a material-selection mistake: an empty texture path on an
authored `BSShaderNoLightingProperty` with shader type 33 is a valid untextured
material. It can use vertex colors and alpha to draw a shadow, marker or effect.
The old adapter colored those meshes magenta. A supported `NiMaterialProperty`
without a texture property also uses untextured diffuse color. Unsupported bindings
still receive a reported magenta diagnostic.

Doc Mitchell's prepared scene has 364 textured and 31 authored-untextured shared
meshes, with zero unsupported diffuse bindings. These counts describe shared meshes,
not the 722 placed mesh instances. The new room captures show window beams and the
cabinet shadow without the previous magenta fallback. Lighting and effect parameters
are still incomplete; this does not establish their retail appearance.

The Rust adapter now carries raw alpha flags/threshold, stencil face draw mode and
shader depth-test/write bits into a separate Bevy extended material. An embedded,
original WGSL fragment shader performs all eight alpha comparisons before blending,
including when both test and blend are enabled. The threshold stays in a uniform;
pipeline keys contain only fixed blend/depth state. Face culling uses the base
material's pipeline key. Texture/sampler sharing is unchanged.

The fixed-state mapping keeps all eleven declared blend factors, including source
alpha, source color, destination color and saturated source alpha. In particular,
`0x1043` darkens the destination with source color, `0x100d` adds the source weighted
by its alpha, and `0x10ed` uses ordinary source-alpha blending. An enabled blend with
an unknown factor is rejected. The saturated factor is one in the alpha channel.
Neither source flags nor decoded geometry are rewritten.

## Asset-free GPU verification

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked -p fallout-preview
.\target\debug\fallout-preview.exe --material-fixture --headless --capture local/material-new.png --report local/material-new.json
```

Use fresh output filenames in an existing directory. This mode does not require an
installation. Its Rust fixture draws a fixed 1280 by 900 grid with known linear
colors, winding and layer depths. The GPU readback is compared with analytic
expectations at each square's center. JSON records source states, expected colors,
measured colors and individual outcomes. A failed comparison exits unsuccessfully
and retains the PNG and measurements. A unit test also rejects a blank readback.

| GPU cases | Count |
| --- | --- |
| Eight alpha functions, each below/equal/above the threshold | 24 |
| Eleven blend factors in source and destination positions | 22 |
| Authored alpha, darkening and additive flag combinations | 3 |
| Simultaneous blending and strict greater-than alpha test | 3 |
| CCW, CW and both-face modes with each winding | 6 |
| Depth test and depth writes, enabled and disabled | 4 |
| Total | 62 |

Expected linear RGB is converted to sRGB and compared with the captured bytes.
The allowance is at most two byte values per channel for render-target quantization;
it is not a retail comparison tolerance. Alpha threshold equality is tested directly,
without an epsilon. Current numeric checks inspect RGB output, rather than separately
certifying the framebuffer alpha channel. See [material-states.json](../reports/material-states.json).

## References and remaining work

Field definitions were consulted in pinned
[nifxml](https://github.com/niftools/nifxml/blob/970a6238218a106daaeb89a61bcda0eeaf9d08c4/nif.xml).
The empty-texture behavior was researched in the pinned
[OpenMW NIF loader](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/components/nifosg/nifloader.cpp)
and [NoLighting fragment shader](https://github.com/OpenMW/openmw/blob/63f6261b6e1fe1eb6170ad4e686de0edec836114/files/shaders/compatibility/bs/nolighting.frag).
These GPL sources supplied format/behavior facts; no upstream implementation was
copied or linked. The pinned MIT/Apache Bevy extended-material API supplies the GPU
host. Exact inspected paths and limits are recorded in [sources.lock.json](../sources.lock.json).

NoLighting angle falloff, external emittance, controllers, property inheritance,
stencil-buffer operations, normal/specular/environment maps and retail lighting
remain open. Retail transparency sorting and placement/camera measurements are also
unverified. Standalone source heading/audio markers remain visible. Collision and
gameplay are not implemented, and no gameplay scenario has been accepted.
