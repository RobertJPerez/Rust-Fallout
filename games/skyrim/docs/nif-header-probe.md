# Skyrim NIF header probe

`skyrim-prep nif-list --archive <v105 BSA> [--prefix <asset path>]
[--offset <0..1000000>] [--limit <1..512>]` enumerates NIF member paths in
archive-table order without opening or decompressing mesh payloads. Offset pages
make large Skyrim archives enumerable without unbounded reports. The result
records the total match count, page offset, bounded returned paths, duplicate
normalized paths, and hash-only/invalid path counts; `truncated` is explicit.
This is useful for choosing known installed Creation meshes before probing their
headers. Header lookup refuses duplicate normalized paths rather than silently
selecting one archive entry. Returned member paths use the shared lookup form
(ASCII lowercase and forward slashes); the header report separately preserves
the chosen entry's actual archive spelling.

`skyrim-prep nif-header --archive <v105 BSA> --member <path.nif>` uses the
existing pinned TES4 archive backend and the project's bounded member reader.
It resolves the member through the shared asset-path normalization, admits the
Skyrim SE/AE BSA v105 dialect, caps one decoded member at 64 MiB, and reports
the exact member spelling, decoded size, SHA-256, text header, and fixed header
tuple fields. The probe does not write extracted retail assets.

The SSE stream tuple is recognized only when the text and binary version are
`20.2.0.7`, the endian marker is 1, User Version is 12, and User Version 2 is
100. The NIF format reference defines the distinct Bethesda stream version as
the `bsstream` field and identifies SSE's version-2 value as 100. The probe's
result means only that this tuple is present. Skyrim LE's User Version 12 /
stream 83 tuple and every other/partial tuple remain distinguishable findings.

The decoder stops before any block data. A recognized header does not prove
that the NIF's block table, geometry, shader properties, skinning, Havok data,
animation, or the Anniversary Edition runtime can consume the file. AE paid
content ownership is not inferred from a NIF or Creation filename. This probe
is a way to build versioned evidence from SE-installed content before broad NIF
work starts; it is not a Skyrim NIF parser.

`skyrim-prep nif-index --archive <v105 BSA> --member <path.nif>` now parses the
SSE outer tables only after the exact 20.2.0.7 / endian-1 / user-12 / stream-100
tuple is present. The pinned NV container API in `fallout-data` accepts
20.2.0.7/user-11 NV streams 14 through 34 and rejects Skyrim's tuple; that guard
and the dependency remain unchanged. The Skyrim-owned adapter bounds counts,
strings and the asset, checks offset arithmetic and type indices, preserves the
upper index bit without assigning Skyrim semantics, and verifies all block
spans end at the parsed root footer with no trailing bytes. Its output includes
the type table, exact opaque block spans, strings, groups and roots. It does not
decode any block payload or references.

The adapter passed synthetic table/truncation tests and six installed samples
from the two base mesh archives and all four Creation archives present in the
measured installation. They range from 4 to 268 blocks; every indexed span table
ends at its footer. This small sample does not certify all SE assets, optional
paid AE content, or runtime compatibility. Offset pages enumerated all 18,862
`Meshes0` and 3,185 `Meshes1` NIF paths. All 3,185 `Meshes1` assets and 18,861
`Meshes0` assets passed; one `Meshes0` member has user-12/stream-83 and remains a
header-only finding. Every NIF path in the four Creation archives present in
this install passed (566 of 566). These installed Creations are not the complete
paid AE set. The NifTools schema at revision
`970a623` identifies the tuple and the field order used by this bounded index;
no schema implementation text is bundled. Broader per-archive validation is
still needed before any block payload adapter is proposed. The indexed base set
and installed Creation set share all 89 observed Creation block names. This is
only a case-sensitive outer-name comparison, not evidence that payload layouts
or runtime behavior are compatible.

The schema also records meaningful SSE inheritance changes: for Bethesda
20.2.0.7, `NiGeometry` changes to `BSGeometry`, and `NiParticlesData` no longer
inherits from `NiGeometryData`. Therefore, even a validated outer table is not
evidence that an NV or FO4 payload decoder is suitable for Skyrim. Keep the
stream-100 schema and later payload support separately versioned. See the
[pinned NifTools schema](https://github.com/niftools/nifxml/blob/970a623/nif.xml#L197)
for the SSE tuple, [header fields](https://github.com/niftools/nifxml/blob/970a623/nif.xml#L1813),
and [Bethesda 20.2 geometry changes](https://github.com/niftools/nifxml/blob/970a623/nif.xml#L3525).

Example invocation:

```text
skyrim-prep nif-header --archive "Skyrim - Meshes0.bsa" --member "meshes\\actors\\character\\character assets\\skeleton.nif"
skyrim-prep nif-index --archive "Skyrim - Meshes1.bsa" --member "meshes\\LoadScreenArt\\LoadScreenMRaltar01.nif" --output local/nif-index.json
```

The format comparison used during this preparation is the
[NIF format reference's version and Bethesda stream definitions](https://github.com/niftools/nifxml/issues/69)
and [SSE stream-version condition](https://github.com/niftools/nifxml/issues/70).
