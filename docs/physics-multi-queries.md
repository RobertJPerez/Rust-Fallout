# Explicit resident multi-model collision

`fallout cell-collision-selection` admits an explicit ordered subset of a cell's
captured model sources. It uses one retained upstream ResidentSources lease and
one immutable scene/index. Every body SHA must match its selected captured model;
archives are not reopened by the collision selection. Source IDs, body frames,
filters, materials, welding, shell exclusions and narrow predicates remain those
of the existing static scene.

```text
fallout --output report.json cell-collision-selection --install INSTALL \
  --load-order order.json --editor-id CELL_EDID --request request.json
```

Optional index/source cache roots use the existing producers and must exist
outside the installation. Output uses the shared protected create-new emitter.
The strict request contains `models`, explicit `units`, `io_deadline_ms`
(1..60000), and `ray` and/or `overlap`. Each model contains `model_index`, exact
`source_sha256`, and nonempty `placements`. Each placement contains a nonzero
integer `reference`, nonempty `body_blocks`, and three four-value binary64
`attachment_rows`. Body references and frames are explicit engineering inputs.
The separate `reference-collision` consumer checks canonical saved reference
identity; this consumer does not construct, mutate or infer canonical World state.

Optional `query_budget` supplies all three total fields: `primitive_tests`,
`geometry_tests` and `hits`. Defaults are 100000, 1000000 and 10000 respectively;
values may be lowered. One or two requested queries divide this total allowance
equally, leaving any remainder unused. Each query uses one combined scene and
index, rather than receiving a fresh budget per model. Rays sort globally by
distance then SourceId; overlaps sort by SourceId. Errors publish no partial
successful report and leave a current runtime selection available for retry.

`physics::multi::CellSelection::admit` takes the original CellResidency/ticket,
ModelSelection inputs, explicit units and lowerable aggregate Limits. It validates
all model indices, captured SHA values and global reference/SHA/body identities
before compiling. Duplicate model indices or duplicate identities refuse. Failed
replacement releases the previous scene/lease and revokes old packets. Opaque
SelectionHits require the original current owner/ticket and a live private
selection lease; public Scope observations cannot authorize admission. Drop,
release, stale access, cancellation/unload and failed replacement retire that lease.

Default ceilings are 64 models, 1024 total body placements, 64 MiB selected source
bytes, 64 MiB aggregate decoded metadata/work, and 16 MiB scope metadata. Existing
scene ceilings apply globally: 100000 decoded NIF blocks, 200000 shape/merge visits,
100000 primitives and 1000000 geometry/index elements. Block accounting includes
noncollision blocks. The counted compiler charges each model's actual usage, then
consumes its leaves into one scene without changing transforms. Both intermediate
model indices and the final combined index consume the same allowance; each merged
leaf consumes a visit. Source/index preflight and final decode metadata are both
charged; allowances never multiply with model count.

Before decoding, the existing NIF index reader admits a bounded preflight. Raw
index scratch is reserved at 32 times source length; graph scratch at source
length times (64 plus longest source type name) bounds opaque-link name cloning.
Scratch must fit half the remaining decode allowance. Fixed decoder tables take
a quarter (4096 bytes per allowed block), and arrays at most a quarter. The
preflight index drops before final decoding. Collision retained metadata and index
buffer/map estimates consume the aggregate allowance afterward. These conservative
engineering reservations may refuse large or uneven sources. Logical accounting
is not a measured process memory peak.

The CLI additionally admits at most 4 GiB of plugin inputs, splits a total four
million indexed records/4 GiB index decode allowance equally across plugins, and
admits at most eight archives/16 GiB. Existing sealed-plan preparation, probe and
residency limits apply, including 64 MiB captured models in this consumer. Request
JSON is capped at 1 MiB. A counting serializer admits the entire pretty report and
newline under 64 MiB before shared emission allocates or opens an output.

Literal plugin/BSA/NIF fixtures establish two sphere sources (radii 1 and 2),
placements at 0/10 and 5/15, and ray entries 2/6/12/16 from -3 along X. Identical
source bytes at distinct paths retain distinct engineering reference attribution.
Packed triangle sources establish entries 3/8/13/18 with exact welding/material/
filter provenance and a shared geometry-test budget of four versus three. Tests
also cover global build/hit/primitive limits, final index costs, late unsupported
sources, forged SHA, duplicate identities, metadata boundaries and owner/lease
revocation with zero model/plan/job charges after unload.

Selected coverage stays Collision Unsupported, `simulation_ready=false` and
`faithful_ready=false`. These queries do not establish whole-cell Havok behavior,
retail units/filter/response policy, saved-pose conversion, actor movement or
original gameplay acceptance.
