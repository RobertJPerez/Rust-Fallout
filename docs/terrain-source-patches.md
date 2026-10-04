# Source terrain patch bundles

`terrain::patches::TerrainPatchBundle::load(directory, store, requests, mounts,
seams, limits)` consumes 1–8 explicitly selected private `CellGridRequest` values.
The result privately owns the existing strict `TextureSourcePlan` for every patch,
with borrowed source receipts, surfaces, meshes and blend maps. Its constructor
and ordered source validation bind CELL, WRLD, grid, cohort, actual LAND header,
decoded body hash and all source field words. Inspector JSON cannot construct it.

The factory first admits every source plan and cumulative output count. It then
uses the existing `reconstruct_cell`, `mesh::build` and `blends::build` helpers.
The immutable CPU outputs retain their existing engineering model names and
source padding/float words. Parent inheritance, retail units, topology, GPU
uploads, material image decoding, collision and activation remain unverified.
Missing, deleted or multiple required LAND entries retain the existing strict
factory refusal. Missing VHGT, invalid normals, unsupported hide bits and invalid
layer order also refuse the whole result.

The complete XCLC flag word is retained. The existing `land_flags` helper uses
its low byte; high bytes remain unused source padding. An absent flag byte is
recorded as absence while the existing engineering mesh model uses zero. That
does not establish an original-engine default. Authored missing/default/ambiguous
material coverage remains in the source plan; CPU preparation never declares
texture/material readiness.

Fifteen lowerable aggregate ceilings cover patches, LAND witnesses, source records,
reads, field sites, retained source metadata and mapped archive extents; vertices,
indices, layers, weight samples, logical output storage, copied metadata, requested
seams and reserved mismatch slots. Nested source-plan limits remain lowerable.
The existing decoder receives remaining aggregate read/field/metadata ceilings.
Source-plan record witnesses and protected archive extents are charged before
retention in the bundle. Archive extents are conservatively charged once per
plan receipt; filenames never imply shared mappings. Temporary source-plan
construction still uses its existing per-plan bounds. These logical limits are
not a process-wide peak-memory or allocator-overhead promise.

Vertex counts and visible triangle-index counts are charged before builders run.
Logical element storage also reserves the existing mesh builder's full index
capacity. Blend weights are charged for every source layer. Every explicit seam
pair reserves all 33 possible mismatch entries before the existing cardinal edge
comparison. Pair order, direction, mismatch indices and both raw height words
remain intact; neither input is smoothed, welded or repaired. Invalid indices,
non-cardinal cells or different worlds refuse.

Patch and bundle identities hash the source plan identity and complete derived
CPU outputs, plus explicit seam requests/results. Admission limits and usage are
excluded so an exact-budget retry retains the same identity. Source plans remain
private, preserving their write-denying archive handles until the bundle drops.

Independent physical plugin/archive fixtures use unequal neighboring cells,
nonuniform height deltas, signed normal bytes, color arrays, hide bits with nonzero
padding, source alpha words and repeated texture references. They verify literal
positions/indices/weights/source spans, all seam mismatch words, caller order,
strict refusal, cohort reuse, protected pin lifetime and every exact/one-under
aggregate bound. No new parser, terrain algorithm, importer, cache or job pool is
introduced. Actual owned CLI and frozen installed-source observation follow the
producer handoff.

