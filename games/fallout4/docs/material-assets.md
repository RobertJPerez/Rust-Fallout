# BGSM/BGEM material asset preparation

Fallout 4 adds material files that carry texture dependencies outside plugin
records. This workspace now has an FO4-only path to inventory those files, parse
the installed corpus with a version-pinned offline reference, and find physical
texture candidates in direct `Data` BA2 archives, and produce a bounded visual
preview of uniquely matched DDS candidates. The shared NV-era BA2 and `AssetPath`
primitives are reused; this does not add renderer, archive winner, or gameplay
behavior.

`extract-materials` scans direct `Data/*.ba2` archives and extracts every member
with a `.bgsm` or `.bgem` extension to a new directory. Its manifest retains
archive/member identity, member-name bytes as seen by the archive backend,
decoded payload hash and size, and a stable archive fingerprint. It rejects
outputs inside the retail install, refuses to overwrite a prior directory, and
writes `complete.json` last. Output is restricted to the workspace's ignored
`local/` directory. Interrupted outputs have no completion marker.

`tools/material-oracle.ps1` checks out no source and edits no reference files. It
requires the isolated, clean `Material-Editor` checkout at the revision recorded
in `sources.lock.json`, compiles its three parser source files into a private
`local/` build, verifies the extraction manifest and payload hashes, then emits
one JSONL result per material into the selected evidence directory. Results
include binary version, exact stream-consumption state, parse diagnostics, and
non-empty texture slots. It also emits typed values for the binary fields exposed
by the parsed BGSM/BGEM model. Five JSON-format startup BGEM files are handled
through the reference library's JSON path; the current Rust comparison keeps
those as texture-only documents. This is corpus research only; it is not a
runtime dependency, and the Rust production library does not use its parser.

The FO4-specific Rust decoder accepts binary version 2 for BGSM and BGEM, plus
the five JSON material documents present in this installation. It caps string
lengths, validates UTF-8, rejects malformed framing and trailing bytes, and
reports unimplemented binary versions explicitly. Its current semantic output
retains named version-2 base, BGSM and BGEM field values alongside texture slots.
Float values stay as exact 32-bit words, booleans retain their source byte, and
RGB colors keep their original component words plus the packed value compared
by the reference model. Across 9,638 binary materials, `verify-materials`
matched 643,587 typed field values, all 32,293 texture slots, and exact consumed
lengths against the pinned reader. The raw per-material field ledger stays under
ignored `local/`. These fields are source observations; they do not assign shader
meaning or establish rendering behavior.

`analyze-material-textures` verifies the oracle revision and result hash, then
checks each authored texture path against physical member names in every direct
`Data/*.ba2`. It applies only a case-folded safe path key and the implicit
`Textures/` root for material-relative paths. Every archive is hashed before
and after indexing. Results keep all candidate archive/member identities and
report collisions and paths with no BA2 candidate; no candidate becomes a
winner. Loose files, active profiles, virtual deployment, archive mounting
rules, creation-plugin activation, and rendering remain unresolved.

`extract-texture-candidates` consumes the completed candidate manifest and
extracts a deterministic sample of up to 24 uniquely matched `.dds` members.
It rechecks the source archive fingerprints before and after reads, binds every
member by exact name bytes and index, verifies the reconstructed DDS dimensions
against the BA2 DX10 header, and caps both per-file and total extraction bytes.
This sample contains 24 textures from four source archives (35,758,856 bytes);
all four archives remained hash-stable.

`tools/build-material-gallery.py` converts those private DDS samples to local
PNG previews with the selected ImageMagick executable. The current run produced
24 of 24 previews with no decoder refusals. The gallery reports the ImageMagick
version and executable hash, each source DDS hash, raw DXGI format value,
dimensions, mip count, material slot names, and candidate archive provenance.
These are unshaded texture previews. They retain channel appearance for visual
inspection and do not establish shader/color-space interpretation or match the
game's renderer. Both raw DDS and PNG files remain under ignored `local/`.

`tools/verify_material_pixel_decoding.py` compares the top mip through two
separate DDS decoders. ImageMagick reads each archived DDS directly; the pinned
Microsoft DirectXTex `texconv` release decodes it to RGBA8 PNG, which
ImageMagick reads into the same raw byte layout. All 24 samples decoded at their
original dimensions. Five matched byte-for-byte and all 24 were within one
8-bit code value per channel; observed maxima were one for DXGI formats 71, 77
and 83, and zero for format 87. This documents decoder agreement within a
one-value rounding tolerance. It is limited to each top mip and does not
validate lower mips, shader/color-space meaning, the game renderer, or VFS
selection. The source revision, executable fingerprint and per-texture output
hashes are recorded in `reports/material-checkpoint.json`.

Reproduce the inspected installed corpus with one compiler job and private
build/cache paths:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bin fallout4-prep --bin analyze-material-textures --bin verify-materials --bin extract-texture-candidates
.\local\target\debug\fallout4-prep.exe extract-materials 'G:\SteamLibrary\steamapps\common\Fallout 4' local\material-extraction-NNN
powershell -NoProfile -ExecutionPolicy Bypass -File tools/material-oracle.ps1 -ExtractionDirectory local\material-extraction-NNN -EvidenceDirectory local\material-oracle-NNN
.\local\target\debug\verify-materials.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local\material-extraction-NNN local\material-oracle-NNN local\material-rust-compare-NNN
.\local\target\debug\analyze-material-textures.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local\material-oracle-NNN local\texture-resolution-NNN
.\local\target\debug\extract-texture-candidates.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local\texture-resolution-NNN local\material-texture-candidates-NNN 24
py -3 tools/build-material-gallery.py local/material-texture-candidates-NNN local/material-gallery-NNN --magick 'C:\Program Files\ImageMagick-7.1.2-Q16-HDRI\magick.exe'
winget download --id Microsoft.DirectXTex.Texconv --version 2026.5.7 --architecture x64 --download-directory local\directxtex-package-NNN --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity
py -3 tools/verify_material_pixel_decoding.py local\material-texture-candidates-NNN local\material-gallery-NNN 'local\directxtex-package-NNN\DirectX Texture Converter_2026.5.7_X64_portable_en-US.exe' local\material-pixel-audit-NNN --magick 'C:\Program Files\ImageMagick-7.1.2-Q16-HDRI\magick.exe'
```

The portable decoder came from the pinned May 2026 DirectXTex release and its
binary hash is enforced by the verifier. Keep the download and research
checkout below ignored `local/`; do not add the decoder to the Rust application
or use it to claim game-renderer behavior.

All raw extracts, per-file reference output, and full candidate reports stay
under ignored `local/`. A generated completion marker establishes that the
bounded offline scan finished; it does not establish runtime texture selection,
native API support, save compatibility, or gameplay readiness. See
[`reports/material-checkpoint.json`](../reports/material-checkpoint.json) for
exact corpus counts and evidence hashes.
