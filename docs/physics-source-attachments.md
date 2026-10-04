# Source-derived collision attachment frames

`fallout collision-attachment SOURCE --request request.json --output report.json`
selects one exact collision-object occurrence, its rigid body and visual target.
The strict request supplies the whole `source_sha256`, nonzero `reference`,
`collision_object`, `body_block`, `target_block`, explicit `placement_rows` and
engineering `units`, plus a `ray` and/or `overlap`. It uses the existing NIF
collision and scene decoders. Source geometry and body/shape predicates remain
those of StaticScene.

`physics::attachment::SourceAttachment::derive` accepts exact source bytes and
Selection, units and lowerable Limits. It verifies SHA before decoding, exact
object/body source spans and hashes, matching nonnull object links, one scene
collision backreference, one collision object for the target, and supported
footer-root ancestry. Required controllers, invalid transforms, repeated roots,
cycles and multiple parents refuse. Collision bodies may be shared by separately
selected objects. Their reports retain distinct whole-source/object/body/target
occurrence identity; the existing Hit SourceId contract is unchanged.

The selected target's existing Scene world transform already contains every
ancestor and the target local transform. The helper composes the explicit caller
placement with that world transform once, using the existing physics composition
helper and central source Affine rows. StaticScene subsequently applies declared
unit factors and the active body pose exactly as before. This does not infer a
runtime saved pose or original Havok attachment behavior.

Opaque blocks could conceal another scene parent. This helper therefore admits
only the union of blocks supported by the existing Scene and Collision readers
and refuses unsupported scene edges. This intentionally conservative scope may
refuse a model with an unrelated unsupported payload. No opaque block is assumed
to have no links, and no visual mesh substitutes for authored collision.

Default ceilings are 4 MiB input, 10000 blocks, 64 MiB decoder reservation, one
16 MiB array allowance split equally across both existing decoders, 200000 source
link visits, 10000 ancestry visits and 16 MiB scope/traversal metadata. Initial
index scratch must fit one quarter of the decoder allowance at 32 times source
length. Before both decoders run, reservation is source length times
`128 + 2 * longest_type_name`, plus 8192 bytes per block and the total array
allowance. This bounds existing index/graph tables and opaque-name amplification.
Metadata reserves 4096 base bytes, 1024 per source block and 1024 per required
ancestor before maps or ancestry entries allocate. These logical reservations
are conservative admission bounds, not measured allocator peaks.

Source link accounting charges every indexed block, decoded collision block,
collision object's two links, ignored collision block in Scene, scene object's
record/controller/collision fields and node child/effect entries, resolved world
transform and footer-root entry. Ancestry visits charge target and each parent
separately. Scope publishes usage and exact selected/ancestor source spans.

Optional request `limits` supplies all seven named ceiling fields. Optional
`query_budget` supplies total `primitive_tests`, `geometry_tests`, `hits`; the
one or two requested queries share this allowance equally, with remainder unused.
Query errors publish no partial successful report. Request JSON is at most 1 MiB;
the complete pretty report plus newline is counted under 64 MiB before emission.
Output must be new and outside the source directory.

The authored fixture has root ID4 above target ID2, noncommuting Z/X/Y rotations,
root/target scale2/0.5, unequal translations and an active half-turn body. Literal
source-world rows are `[0,0,1,2]; [1,0,0,26]; [0,1,0,40]`; caller composition gives
`[0,1,0,140]; [1,0,0,226]; [0,0,-1,298]`. Explicit Havok factor2 and query factor3
give box center `(432,684,876)`, half extents `(12,6,18)` and ray entry20 from
`(400,684,876)` along X. The unchanged slab query returns its conservative
representable entry witness; tests bound the tiny binary64 rounding difference.
Separate shared-body selection proves exact occurrence identity. Exact source,
decoder, array, link, ancestry and metadata boundaries and uncertain source
refusals are tested through literal NIF bytes.

The frozen consumer proof passed 48 authored cases: eight successes and 40
refusals. Two authored sources also matched the separate pinned offline NIF
oracle's scene matrices and selected collision spans. The selected 3576-byte
installed source (SHA `edb671e804fc3e062a4ed5a9b158cb3283304121efbb0e7ecbc0b92ba193cc65`,
object5/body4/target0) retained an explicit opaque-block ancestry refusal; its
offline native source observations were preserved. It establishes no positive
original attachment acceptance. Source files and frozen executables stayed
unchanged throughout this proof.

All results remain engineering source observations, `faithful_ready=false`.
They establish no original units, filter/response policy, dynamic controller,
whole-cell simulation readiness, actor movement or gameplay acceptance.
