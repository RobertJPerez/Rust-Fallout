# NIF materials and texture dependencies

The Rust scene decoder now reads nine additional payload types: `NiMaterialProperty`,
`NiAlphaProperty`, `NiStencilProperty`, `NiShadeProperty`, `BSShaderPPLightingProperty`,
`BSShaderNoLightingProperty`, `BSShaderTextureSet`, `NiSourceTexture`, and
`NiTexturingProperty`. These are typed source fields, not a Bethesda shader implementation.

Version-dependent fields remain optional instead of being filled with invented defaults.
The decoder retains shader and alpha flags, original texture slots, empty slots,
texture transforms, shader maps, source-image metadata, and raw path bytes. Supported
blocks must consume exactly their declared extent. Property links to texture sets and
source textures require the correct target type. The existing scene allocation budget
also covers derived path storage, including repeated string-table references.

External paths normalize ASCII case and slash direction, preserve non-ASCII bytes, and
gain a `textures/` prefix when absent. Absolute paths, traversal, control bytes, and
paths over 4,096 bytes fail resolution. Original bytes and errors remain in the report.
Embedded pixel references stay distinct from external dependencies.

`ArchiveAssets` holds immutable handles to the discovered archives and lists every
candidate before reading. Only a unique candidate is usable: production override order
and loose-file precedence remain unfinished. Archive digests are calculated lazily and
memoized while those handles remain open. The texture probe deduplicates dependencies,
retains every model/block/slot usage, and verifies cache publication and reuse.

## Verified scope

- Raw nifly comparison: 218 files and 1,127 material blocks agree, alongside the
  previously decoded scene objects and mesh attributes. Source floats compare by f32
  bits; references, flags, texture slots and path bytes compare exactly.
- Eight material types occur in that independent sample. `NiShadeProperty` has a
  synthetic fixture but no independently compared retail occurrence.
- Doc Mitchell's 207 models contain 1,120 material blocks and 854 external texture
  references. They resolve to 312 unique, unambiguous archive paths. All 58,415,256
  extracted texture bytes have cache manifests and verified payload hashes.
- The full scan decodes 176,982 material blocks and 139,266 external references in
  25,729 files. The same six legacy containers and 25 strict scene failures remain.
- Seven authored paths are absolute exporter paths under `d:\projects\fallout\...`,
  across six DLC effect/projectile models. They remain unresolved; no path-stripping
  compatibility rule is assumed. The census retains all seven diagnostics.
- Seven deliberate comparator mismatches are detected: vertex float, winding, world
  translation, source digest, material alpha, texture path, and missing material data.

See [material evidence](../reports/nif-materials.json) and
[per-type coverage](../parity/nif-material-coverage.json). The full census is a parser
scan; independent field comparison covers the sample above, not every archived model.

## Reproduce

Build the CLI and prepare the cell model cache as described in
[world inspection](world-inspection.md). The cache directory must already exist outside
the game installation; a bad destination is rejected before starting the batch.

```powershell
New-Item -ItemType Directory -Force local\docmitchell-textures | Out-Null
.\target\release\fallout.exe nif-assets local\docmitchell-models --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --texture-cache local\docmitchell-textures --output local\textures-new.json
```

The scene/oracle commands in [scene decoding](nif-scenes.md) now include material
fields. Rebuild the separate C++ oracle first: an older report without its material
projection cannot certify these fields. Neither the GPL oracle nor schema-generated
code is linked into the Rust programs.

The decoder preserves normal, glow, environment and other authored slots without
claiming their rendering semantics. Property inheritance, controllers, skinning,
collision, retail shaders and complete asset dependency closure remain open.
