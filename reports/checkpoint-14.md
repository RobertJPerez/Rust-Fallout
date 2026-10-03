# Implementation checkpoint 14: authored diffuse terrain preview

The inspection viewer now draws the verified terrain texture chain with authored
quadrant byte weights. Shared diffuse images feed separate opaque base and additive
overlay passes. Missing bases and NULL defaults reject textured rendering explicitly.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 136 tests and Clippy with warnings denied |
| Source/weights | Six base/DLC cells; selected fields, geometry and all 31,212 weight bytes match |
| Texture archive bytes | All 40 unique selected members match the independent ba2 reader |
| Textured GPU captures | Goodsprings, GoodspringsSource and NVDLC02PineCreek |
| Regression captures | TownCenter vertex colors and 400-reference Doc Mitchell interior |
| Numeric GPU checks | All 64 original expectations pass, including two textured-weight cases |
| Test launcher | Rust launcher and machine-local configuration validated without UI input |
| Incomplete material cases | TownCenter and two default-layer DLC fixtures reject before rendering |
| Cache/source safety | Copied corruption rejected; original installation hashes unchanged |

The terrain preview uses explicit inspection tiling and unlit additive compositing.
This does not establish retail blend/normal/specular shaders, color space, lighting,
defaults, physics, streaming or gameplay. All faithful scenarios remain unaccepted.

The first direct visual/input test is ready: see
[the build and test instructions](../docs/terrain-textured-preview.md).
Retail UI automation could not run because the Windows helper's native pipe was
missing after retry and reset. Headless GPU tests ran independently of that helper.

[GPU metadata](terrain-textured-preview.json) and checkpoint-specific source and
verification reports bind actual inputs and executables. Captures remain local.
Earlier immutable evidence remains unchanged. M1 and gameplay milestones stay open.
