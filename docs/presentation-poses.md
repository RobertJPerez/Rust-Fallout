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
animated palette. Explicit finite-time object poses are a subsequent consumer.

Independent analytic tests cover transformed points versus directions, one world
and basis application, cancellation in binary64 before narrowing and overflow
refusal. The tested handoff retains a separate frozen producer report, an
independent draw-array calculation, selected-source GPU capture, intentional
refusals and default report compatibility. Engineering rendering and original
gameplay acceptance remain separate.
