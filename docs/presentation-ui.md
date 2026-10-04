# Retained original menu source

The preview's `--menu menus/options/start_menu.xml --install INSTALL --report
LOCAL_REPORT.json` path reads the exact unique archive member through the existing
`ArchiveAssets` importer. `--menu-tile NAME` selects one exact raw authored `name`
attribute; missing or repeated names refuse instead of selecting the first match.
The source inspection finishes without creating a renderer or evaluating tiles.
This is the retained source input for the original menu consumer; original display
and VIEW05 gameplay acceptance remain open.

The local report binds the archive member, archive SHA-256 and payload SHA-256. It
retains the entire exact UTF-8 source, including a BOM if present, and a forest of
nodes with half-open byte spans. Elements retain opening/closing spans, ordered
raw attributes, parent identity and every child in source order. Text, whitespace,
custom entity references, CDATA, comments, declarations, processing instructions
and DTD declarations remain source observations. Includes, templates, source-local
traits, nested operators and authored action IDs retain their original hierarchy;
there is no synthesized root or copied default layout. Node indices identify this
source document and are not runtime/canonical IDs.

The audited quick-xml tokenizer operates only on already admitted bytes. It never
opens an include, fetches a DTD or resolves an entity. Attribute values remain raw;
`&name;` does not become a guessed boolean, localization string or trait. Unknown
operators are retained in order. No operator arithmetic, trait defaults, include
expansion, font choice, focus navigation or action dispatch is accepted by this
slice. `original_display_ready` is always false. Reports contain local raw retail
source and must remain local.

Admission is bounded to 1 MiB of source, 32,768 events including EOF, 16,384 nodes,
128 element levels, 64 attributes per element and 4 MiB of charged logical retained
metadata. Metadata is charged before node/attribute/edge retention. This measures
declared logical storage, not tokenizer temporary or allocator peak memory.
UTF-8 and ASCII declarations are supported; other declared encodings, invalid
UTF-8, mismatched/unclosed tags and duplicate attributes refuse explicitly. Source
divider comments containing internal `--` are retained as opaque text, matching
the original menu input; an unclosed comment still refuses. This is a source UI
tokenizer, not a claim of strict W3C XML conformance. Source
fragments may contain several top-level declarations/elements as original UI
templates do. They retain their source order instead of acquiring a fake root.

Reports stream through an 8 MiB output ceiling and use `create_new`, outside the
installation. An interrupted or failed report can remain partial and immutable;
it is not a completed inspection. A successful report proves retained source
structure, never evaluated original menu rendering. The existing original
source/operator/font review remains authoritative for subsequent trait and font
work; this implementation does not repeat that audit.
