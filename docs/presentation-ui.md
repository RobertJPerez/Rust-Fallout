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

The opt-in --menu-includes REQUEST.json reads an exact include dependency
closure. The strict schema-1 request supplies the root member path/archive/payload
SHA-256 and one binding for every include source-value span: parent member path,
parent payload SHA-256, src_span { start, end }, exact raw_src, and a target
with exact member path/archive/payload SHA-256. Bare source names such as
list_box.xml require this explicit binding; the consumer does not infer a
directory, search order, loose-file override or template inclusion rule. Only
literal src and empty/whitespace/comment include bodies are admitted. Unknown
fields, custom entities in paths, duplicate/unused bindings, changed source hashes,
missing or ambiguous members and unsafe paths refuse the complete request.

The closure retains separate source-qualified Documents and exact include edges.
Duplicate dependencies share their immutable document. Iterative traversal reports
cycles with member/value-span routes and bounds the full dependency depth even
when a longer path reaches a reused subtree. No nodes are spliced, templates
instantiated or operators evaluated. A selected authored name still belongs to
the root source; original_display_ready remains false.

Closure limits are 64 files, 256 edges, 16 include levels, 4 MiB aggregate source,
131,072 events, 65,536 nodes and 16 MiB declared logical retained metadata, with
the existing per-document limits. Remaining budgets constrain each archive read
and parse before retention. The caller request is limited to 128 KiB and the
closure report to 32 MiB through the existing bounded serializer. Working request
maps/DFS state and tokenizer temporary allocation are bounded separately, without
claiming allocator peak memory. Failure returns no closure and the CLI does not
create its report until complete dependency admission; a subsequent output/write
failure can still leave a partial immutable report. Default single-file --menu
output and original UI evaluation/font/display acceptance remain unchanged.

The opt-in `--menu-entities REQUEST.json` observes one exact source value. Its
strict schema-1 request binds `source { path, archive_sha256, payload_sha256 }`, a
`selection`, and an ordered `definitions` array of literal `name`/`value` pairs.
Selection is either `element-text` with `node` and the complete element `span`, or
`attribute` with its owner `node`, exact `name_span` and `value_span`. Existing
node/span identities must match. This request carries its own selection and cannot
be combined with the include-closure or authored-name selector flags.

Element text admits direct text, entity references and CDATA; comments contribute
no text and nested markup/operators refuse. Attribute observations retain their
literal whitespace. XML builtin names use the pinned tokenizer's five-entry
resolver, and numeric references use its Unicode character-reference API. Numeric
validation follows that API's documented behavior, including zero/surrogate/range
refusal; complete XML LegalChar checking remains outside its verified scope.
There is no DTD fetch, attribute whitespace normalization or localization rule.

Custom entities require an explicit value bound to the source payload. Duplicate
definitions, builtin/numeric overrides, NUL and entity-shaped nested replacements
refuse. Values are literal UTF-8; a plain ampersand and an explicitly empty value
are supported. Unsupplied names produce `value: null` and every exact source span
in `unresolved`. A resolved empty string remains distinct from that unavailable
result. Missing values never publish a partly expanded string.

Each occurrence and projected replacement byte is admitted before output copying,
including repeated references and empty replacements. Limits are 128 KiB request,
128 definitions, 64 KiB environment names/values, 32,768 pieces, 16,384 references,
1 MiB selected input, 1 MiB expanded UTF-8, 2 MiB declared logical working metadata
and a 12 MiB streamed report, alongside existing source-document limits. Definition
names are at most 128 bytes; source reference names are at most 256 bytes. Copies
begin after complete preflight. This is an explicit text observation for the later
tile consumer; layout, operators, typed traits and original display remain open.
