# Implementation checkpoint 06: authored materials and GPU render states

Doc Mitchell's house now renders its authored untextured window and shadow meshes
without the old magenta fallback. The inspection adapter preserves source alpha
comparisons, blend factors, face culling and depth behavior. Geometry, plugin
placements, shared asset storage and the strict source decoder stay intact.

| Check | Result |
| --- | --- |
| Workspace checks | Formatting, 54 tests and Clippy with warnings denied passed |
| Preview build | Windows MSVC debug executable built and exercised |
| Synthetic GPU comparisons | All 62 pass: alpha, blending, culling and depth |
| Blank capture guard | Unit test rejects empty readback and retains failed measurements |
| Real GPU captures | Two models and two interior views completed and visually inspected |
| Shared material bindings | 364 archived textures, 31 authored untextured, zero unsupported diffuse bindings |
| Interior instances | 400 references, 203 shared models, 722 mesh instances |
| Shared geometry | 96,394 vertices and 84,378 triangles before instancing, unchanged |
| Shared texture/samplers | 146 pairs, unchanged |
| Selected records | 435 references, 222 bases, zero integrity/link failures |
| Original installation | All 464 files freshly hashed and match the original baseline |
| Distribution | Source and evidence metadata uploaded to RobertJPerez/Rust-Fallout; retail assets remain local |

An empty filename in the supported NoLighting shader describes an untextured
surface. The previous viewer treated it as a missing diffuse texture and applied
magenta. The new binding report distinguishes authored untextured surfaces from
unsupported bindings. Real captures now show the cabinet overlay, window and light
beams through the supported diffuse/vertex-color path.

The fixed-state adapter uses an original embedded WGSL alpha test and Bevy extended
material specialization. It preserves all eight comparison functions and eleven
blend factors. Both blending and alpha testing can operate together. Source flags
`0x1043`, `0x100d` and `0x10ed` retain darkening, additive and source-alpha behavior.
Thresholds remain uniforms, while pipeline keys contain only fixed states. Unknown
enabled blend factors are rejected rather than approximated.

The asset-free Rust fixture checks a known grid through actual GPU readback. It
tests threshold equality directly, both face windings and overlapping depth layers.
Expected linear RGB is converted to sRGB; the maximum allowance is two byte values
per channel for render-target quantization. This is not a retail image tolerance,
and framebuffer alpha is not independently certified. The measurements and source
states are in [material-states.json](material-states.json); reproduction commands
and source references are in [material states](../docs/material-states.md).

Pinned nifxml field definitions and selected OpenMW NoLighting sources informed the
format/behavior research. No OpenMW C++ or GLSL implementation was copied or linked.
The separate Rust data decoder is unchanged. The 218-model raw nifly comparison and
full/deferred selected-field comparison are retained checkpoint 05 evidence, bound
to the unchanged release CLI; they were not rerun or presented as a new corpus scan.

This remains an unlit inspection runtime. NoLighting angle falloff, emittance,
inherited properties, controllers, stencil-buffer operations and retail lighting
remain incomplete. Standalone heading/audio markers remain visible. Effective
retail profiles, archive/loose precedence, original format defects, measured placement,
collision, player simulation, scripts and saves remain open. No gameplay scenario
has passed acceptance. See [NEXT_STEPS.md](../NEXT_STEPS.md).

Checkpoint 05 evidence is preserved in [its verification](checkpoint-05-verification.json),
[source snapshot](checkpoint-05-source-snapshot.json),
[interior captures](checkpoint-05-interior-preview.json) and
[model captures](checkpoint-05-model-preview.json). Current source and executable
hashes are recorded in [verification.json](verification.json) and
[source-snapshot.json](source-snapshot.json). An implementation commit precedes the
evidence commit so the reported revision identifies the tested source.
