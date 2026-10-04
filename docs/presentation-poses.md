# Explicit source skin inspection

The existing model inspector can display one exact source skin geometry through
the production `nif_skin::pose::evaluate` API. The request names an archived model,
its geometry block and an explicit absolute weight-sum tolerance:

```powershell
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --model meshes/armor/1950stylecasual01/f/outfit.nif --skin-geometry 92 --skin-weight-tolerance 2.384185791015625e-7
```

The paired skin arguments require model mode. Only the selected geometry reaches
the ordinary mesh, UV, material, texture and bounded upload adapters. There is no
second skin decoder, link resolver or palette evaluator in presentation code.
Default model and CELL inspection retain their existing static mesh selection.

The producer evaluates stored source locals and raw nonnegative weights. The
renderer applies `skin_to_source_world` to its evaluated positions in binary64,
then applies the shared `[x,y,z] -> [x,z,-y]` basis and narrows to binary32. It
never applies the geometry owner's matrix again. Normal directions use the
same linear map with translation excluded; their raw weighted length is retained,
without normalization or inverse transpose. This is an unlit inspection view.
The existing winding rule uses the final source-world matrix determinant. A
singular, nonfinite or unrepresentable draw transform refuses admission.

The opt-in model report uses schema 3. `source_skin_pose` records the exact source
hash and block IDs, requested weight policy, world matrix, vertex/palette counts,
raw weight-sum range and unapplied controllers. It also hashes the exact draw
positions and normals in attribute order as little-endian binary32 triples. The
adapter verifies those hashes against the actual mesh attributes before upload.
Default model reports retain schema 2 and omit this field.

Unsupported skin links, raw weights outside the caller's tolerance and selected
geometry omitted by the existing visibility/material adapter produce explicit
loading failures. They cannot become a ready capture. Missing command arguments
fail command parsing. Stored controller fields remain unapplied; this slice does
not establish a retail bind pose, playback, actor equipment assembly or a whole
animated palette. Explicit finite-time object inspection is described below.

Independent analytic tests cover transformed points versus directions, one world
and basis application, cancellation in binary64 before narrowing and overflow
refusal. The tested handoff retains a separate frozen producer report, an
independent draw-array calculation, selected-source GPU capture, intentional
refusals and default report compatibility. Engineering rendering and original
gameplay acceptance remain separate.

## Explicit source-time object inspection

`--pose-object BLOCK --pose-controller BLOCK --pose-time TIME` supplies an exact
same-container request to the existing `nif_animation::pose::evaluate` producer.
All three arguments and one model source are required. Skin selection and object
selection are mutually exclusive. Time is binary64 in the source key domain;
the host supplies no animation clock or frame-time substitution.

The selected object's returned `source_world` already includes its source static
ancestors. Its static descendants compose their declared local transforms after
that matrix using the renderer's binary64 matrix transport, without applying
the old selected-object world matrix. Additional
descendant controllers and skin palettes refuse. Draw hierarchy admission limits
are 16,384 objects, 32,768 work units and depth 1,024. Singular or unrepresentable
draw matrices also refuse even when the producer accepts a zero forward scale.

Positions and raw normal directions use the same binary64 world/basis transport
as selected skins. The opt-in schema-3 `source_object_pose` keeps the producer's
exact identity, spans, time bits, raw controller/interpolator fields, channels and
static ancestry, plus each admitted mesh's final world matrix and draw hashes.
Stored NiAV visibility and existing material inspection remain in effect.
There is no claim of original rotation-key, clock, event or playback behavior.

`--model-file PATH --install PATH` admits an explicit NIF file, capped at 64 MiB,
through the same production decoder and model adapter. Its diffuse paths still
resolve through the existing archive lookup. The report explicitly names the
source file; its `local-source/...` model label is not an archive entry. Archived
model mode and default schema 2 remain unchanged. This also makes bounded authored
fixtures reachable through the actual loading, draw and GPU capture path.
Capture and report paths must be outside both the installation and the explicit
source-file directory; existing outputs are refused.

`tools/presentation-oracle/pose_draw.py` authors two tiny source packets without
game bytes. The checked-in `.packet` fixtures include a static quarter-turn
parent, explicitly sampled half-turn object, transformed triangle child, colors
and an authored two-sided material. Literal expected matrices and vertices expose
transform ordering, signed-scale winding and raw normal length. The second packet
adds a descendant controller that the renderer must refuse. All retail-derived
inputs, captures and per-source reports remain under ignored local evidence.

## Explicit source-time skin inspection

Add all five sample arguments to an existing `--skin-geometry` and
`--skin-weight-tolerance` model request:

```text
--skin-sample-object BLOCK --skin-sample-controller BLOCK --skin-sample-time TIME
--skin-sample-source-sha256 HEX64 --skin-sample-controller-policy refuse-other-required
```

The source hash is exactly 64 hexadecimal ASCII digits. The existing published
`nif_skin::pose::evaluate_sampled` producer validates that source, the exact
same-container object/controller/time links, selected skin ancestry and the
explicit other-controller policy. Presentation adds no sampler. Missing joints,
wrong hashes/links, unsupported channels, invalid times or another required
controller refuse the complete source request. The unit-weight tolerance remains
explicit and never repairs weights. The producer's existing conservative combined
decoder, element-storage and work allowances apply.

Validated positions and raw normal directions use the same existing binary64
skin-world/basis transport, mesh attribute hashes, materials and bounded upload
path. The geometry owner's transform is excluded. The optional schema-4 report
keeps `source_skin_pose` and adds `source_sampled_skin_pose`: the full validated
sample receipt, source palette, explicit controller policy and producer admission
counts. Charged element accounting includes scratch already released and is not
a measurement of retained process memory. Raw deformed source arrays are released
after draw conversion; the palette moves into the receipt without a second copy.

With no sample arguments, stored-pose schema 3 and default schema 2 reports retain
their previous fields and behavior. The host applies no running animation clock,
rotation-key mapping, original callback or playback policy. Original animated
skin rendering and gameplay remain unaccepted.
