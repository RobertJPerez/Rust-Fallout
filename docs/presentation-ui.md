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

The opt-in `--menu-traits REQUEST.json` projects one exact named tile's direct
source fields. A strict schema-1 request binds `source` as above, `tile { name,
node, span }` and explicit `conversions [{ name, kind }]`. The name must select a
unique existing tile and match its node/span. Conversion kinds are `string`,
`finite-f32` and `boolean-01`; their assignment is caller input, not a guessed
runtime type for a source trait. This flag has its own selection and conflicts
with the other menu selector/consumer flags.

All direct element rows retain source order, name and whole/inner spans, plus
exact inner source spelling. The complete direct child index list and source
Document retain non-element nodes too. Requested absent names follow source
rows, with null spans/spelling and `absent` status. Tile/include/template children
remain structural. Duplicate declarations, attributed declarations, nested
operators, unsupplied conversions and custom entities remain unresolved. Builtin
and numeric references reuse the entity consumer with no custom definitions;
malformed references refuse the complete projection. No field precedence,
template expansion, arithmetic, visibility or layout default is supplied.

Strings preserve literal whitespace, including a resolved empty string. Numeric
and boolean policies trim only XML ASCII whitespace and preserve an `empty`
result distinct from absence. Finite numbers use Rust's f32 parser and publish
both the value and exact f32 bits, preserving negative zero. Malformed numbers,
nonfinite numbers, overflow and a nonzero mantissa rounded to zero have explicit
unresolved reasons. Other finite rounding follows that parser. Boolean conversion
admits exactly `0` or `1`; custom `&true;`/`&false;` entities require future explicit
source values and do not select a truth value here.

Limits are 128 KiB request, 128 conversions, 256 rows, 2 MiB reserved copy bytes,
1 MiB declared logical projection metadata and 16 MiB streamed output, plus
existing Document/entity limits. Before row/value copying, the complete plan
reserves each copied name plus twice its inner UTF-8 bytes, conservatively covering
raw spelling and decoded value or missing-reference names. Metadata accounts for
the borrowed plan/maps, owned row and reference structs and complete direct child list, excluding
allocator peak and temporary entity work already bounded by that consumer.

`--menu-dependencies REQUEST.json` is a separate opt-in inspection mode with
installation and report paths. Its strict schema-1 request lists exact `sources`
(path/archive SHA/payload SHA), explicit `bindings`, initialized opaque `inputs`
and ordered `changes`. Each endpoint identifies its source index, node, complete
span and name span. A binding identifies the target's exact operator node/span,
plus the exact `src` and `trait` attribute name/value spans. The source trait name
must match that literal trait operand. The operator must lie inside the target
trait, with only known operator ancestors in between. Endpoints must be direct
traits of existing tiles. Sources can span several documents; no XML reference
path or template relationship is inferred from the caller's binding.

The session builds source-to-dependent reverse edges once. Repeated edges collapse
while the original request retains every operand binding. Conflicting assignments
of one operand to different endpoints refuse; identical repeated assignments
remain allowed. Iterative DFS refuses a
cycle with a closed source-index/node/span witness before session publication.
Affected work uses deterministic reverse DFS postorder; ties follow ordered source
and node identities. Each diamond descendant appears once. Changed inputs appear
separately from downstream work, and unrelated expressions stay untouched.

Each change carries the cohort SHA, expected revision and exact endpoint/value
updates. The cohort hashes a fixed domain tag, a little-endian u64 source count,
then each ordered path/archive SHA/payload SHA as little-endian u64 byte length plus
UTF-8 bytes. Initial values and updates are explicit opaque caller input, including
empty strings; they are not evaluated source expressions. No-op changes leave the
revision and work unchanged. Full identity, value, traversal, report-row and checked
revision admission precede value copies/state replacement. A refused change leaves
all input values and revision intact; complete batch value sizing is independent
of update order.

Limits are 256 KiB request, existing include source-file/aggregate Document limits,
1,024 graph nodes, 4,096 bindings, 1,024 initialized inputs, 64 KiB per input and
1 MiB current opaque values, 128 changes and 128 updates per change. Validation is
limited to 32,768 steps and 8 MiB source operand/name bytes. Each propagation admits
at most 16,384 node/edge visits. Declared logical session metadata is limited to
4 MiB; complete CLI results admit at most 16,384 changed/affected rows before
retention, with a 32 MiB streamed report ceiling. Source Documents and bounded
request strings have their own limits. A later report write failure can leave a
partial immutable output; semantic batch failures create no report. This identifies
work in the explicit graph, without arithmetic, engine defaults, original menu
display or gameplay acceptance.

## Literal rectangle inspection (VIEW20)

`fallout-preview --install INSTALL --menu-rectangles REQUEST.json --report REPORT.json`
opens a fixed inspection viewport. Add `--headless --capture CAPTURE.png` for an
engineering GPU capture. The mode requires a strict schema-1 request with the
exact source path/archive SHA/payload SHA and selected unique root
`tile: { name, node, span }` from the retained source report. It accepts no model
mode or source-camera override. Request admission occurs before graphics startup;
archive reading and XML projection start after the window event on the existing
preparation worker.

The caller must supply `policy: "parent-relative-pixels-rgba255"`,
`viewport: { width, height, background: [r,g,b,a], depth_range: [min,max] }`, and
`parent: { origin: [x,y,depth], opacity, visible }`. Background uses unit sRGB;
parent opacity is in 0..1. Every selected rectangle and descendant must explicitly
declare finite-f32 `x`, `y`, `width`, `height`, `depth`, `red`, `green`, `blue`,
`alpha`, and Boolean01 `visible`. Positive dimensions, RGB/alpha in 0..255,
exact source spans and numeric float bits are retained. Only optional source
`name` attributes, rectangle children, comments and XML ASCII whitespace are
accepted alongside these fields. Absent/empty/duplicate/attributed fields,
operators, custom references, other tile kinds and other direct traits refuse.
The existing literal projection and entity resolver remain the only conversion
path; child rectangles bind exact topology even when a child name also appears
outside the selected subtree.

This is an explicit caller inspection policy: local x/y add to the parent in
right/down pixels; depth adds with larger values in front; visibility is ancestor
AND; straight opacity is the ancestor product times source alpha/255. It does not
establish original engine units, defaults or arithmetic. Geometry maps y to world
-y and draws unlit quads through the existing material and bounded upload queue.
The orthographic camera and initial physical pixel extent stay fixed; the window
cannot be resized and camera movement is disabled in this mode. Escape/close and
loading cancellation retain their existing host boundaries. Transparent colors
blend in linear space through the existing source-alpha factors; RGB PNG captures
do not establish output-alpha channel behavior. Overlapping visible nonzero-alpha
rectangles at equal f32 depth refuse because this consumer has no certified tie
ordering policy. Collapsed geometry, draw area, reconstruction and subnormal GPU
coordinates/color/opacity also refuse.
Distinct world depths can round to one camera-space sorting key. The consumer
also rejects overlapping visible positive-alpha draws with identical keys from
the pinned Bevy `ViewRangefinder3d`; each plan retains that key and its bits.
The world-depth equality refusal stays in place. This avoids claiming an order
when the actual transparent sort cannot distinguish the two draws.

Limits are 128 KiB request, 256 rectangles/draws, subtree depth 64 and 32,768
direct traversal visits. Aggregate literal projection reserves at most 8 MiB
copies and 4 MiB logical metadata, with 1 MiB plan/stack metadata and 256 KiB mesh
submission bytes. Coordinates are limited to magnitude 1,048,576 pixels. The
viewport admits 1..4096 pixels per dimension and at most 4,194,304 pixels total;
finite ordered depth endpoints and their span are similarly bounded. Existing
source Document caps apply. Full source/request/plan reports have a 16 MiB ceiling.
These are logical admission limits, not a peak allocator or VRAM measurement.

Each instance and mesh carries the same immutable archive/payload receipt,
source node/span/root and current scene epoch. Source topology remains in the
report; draw instances use resolved positions and inherited visibility. Labels
retire alongside the existing bounded entity/resource queue. Cancellation retains
a late prepared result until the host drains it; stale epochs cannot publish it.
The source report is a snapshot read during preparation, without a continuously
held source-file lease. Retry verifies the same explicit source identity again.
Semantic refusal precedes report creation; a later I/O or cancellation boundary
can leave a fresh diagnostic report without a capture. Original menu readiness,
text/fonts, template evaluation, UI focus/actions and gameplay remain unaccepted.

## Selected literal image inspection (VIEW21)

`fallout-preview --install INSTALL --menu-image REQUEST.json --report REPORT.json`
connects one exact source image tile to the existing DDS decoder and bounded draw
queue. Add `--headless --capture CAPTURE.png` for a GPU capture. The strict
schema-1 request supplies XML `source` and DDS `texture`, each with full member
path, archive SHA and payload SHA. Select `image: { node, span, name }`; `name`
may be null or omitted, but a supplied name must uniquely identify that node.
The literal filename must normalize to the supplied full `textures/... .dds`
member. Relative paths have no prefix or fallback.

The request explicitly supplies the rectangle caller policy, viewport and parent
described above, `uv_rect: [u0,v0,u1,v1]` in ordered finite 0..1 coordinates, and
`sampling: "nearest-clamp-edge-mip0"`. All nine numeric layout/color fields,
Boolean01 visibility and literal filename are required. Only an optional name
attribute, those fields, comments and XML whitespace are admitted. Expressions,
custom references, nested tiles and other traits refuse, including repeat,
texture-atlas, crop, rotation and file-dimension traits even when their value is
zero. These caller policies do not establish original image semantics.

The existing adapter retains BC1/BC2/BC3 sRGB compressed mip payloads; this mode
uses nearest filtering, clamp-to-edge and LOD zero. Reports retain the full XML
tree, literal spans/numeric words, normalized filename, explicit UV words, actual
camera sorting key, DDS provenance, dimensions, format, mip count and byte/texel
counts. Instance and mesh labels share XML and DDS receipts with the scene epoch;
stale work cannot publish and owned resources retire through the existing queue.
The fixed viewport disables resizing and camera movement. Archive/DDS work
starts after the window event on the preparation worker.

Admission allows one 152-byte quad, 128 KiB request and 16 MiB report, at most 32
literal rows, 4 KiB resolved filename, 16 KiB retained filename copies and 64 KiB
logical plan metadata. Existing document, projection and viewport caps apply.
DDS input and retained payload each have an 8 MiB ceiling; base and aggregate
mip texels must fit 4,194,304 (16 MiB virtual RGBA). These counts describe logical
admission rather than measured peak memory. Complete report serialization is
admitted before creating a fresh report; later I/O or cancellation can leave a
diagnostic report without a capture. Source receipts are preparation snapshots,
with identity checked again on retry. Original menus, fonts, focus/actions and
gameplay remain unaccepted.
