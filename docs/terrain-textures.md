# Terrain texture dependency inspection

The terrain command now follows authored nonnull LAND layer references through
winning LTEX and TXST records to unique archive members. Every selected record
retains its defining plugin, original header, decoded offsets and source/body hashes.
The inspection keeps asset candidates, original path bytes, archive hashes, decoded
byte counts and optional verified cache receipts. Texture pixels and blending are
separate future stages; the current Bevy terrain preview still uses vertex colors.

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli -p archive-compare
New-Item -ItemType Directory -Force .\local\terrain-bodies-new,.\local\terrain-textures-new | Out-Null
.\target\release\fallout.exe terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --inspect-textures --body-cache .\local\terrain-bodies-new --texture-cache .\local\terrain-textures-new --output .\local\goodsprings-textures-new.json
~~~

The stage is opt-in and independent of --reconstruct-heights/--inspect-mesh, which
can be combined with it. Use a fresh report name. Cache roots must exist outside
the installation. No source file, profile or save is modified.

## Source fields and resolution

[Pinned xEdit FNV definitions](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
describe LTEX EDID/ICON/TNAM/HNAM/SNAM/GNAM and TXST EDID/TX00 through TX05/DNAM.
The original Rust decoders retain Havok bytes, specular exponent, ordered grass
links, path fields and flag bits. OBND, DODT and other unhandled subrecords remain
raw bytes; their behavior is not implemented. Known singleton fields reject
duplicates and invalid lengths. Paths require one final NUL and at most 4,096
authored bytes. Absence and an authored empty string stay distinct.

Each field resolves through its defining plugin's master table. Winning records
replace complete bodies; paths from older definitions are not merged back in.
Bindings retain missing, null, deleted and wrong-kind targets. Missing TNAM or
diffuse TX00, bad nonnull links, unsafe paths, ambiguous/missing members and cache
failures produce diagnostics and CLI exit 1. Grass references receive kind checks;
grass payloads, assets and rules are outside this texture chain.

The [shared xEdit LAND schema](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsCommon.pas)
permits NULL in BTXT/ATXT and labels the NV default as LDirtWasteland01. Four such
layers occur in the selected DLC fixtures. Their status is null-default-unapplied;
they do not invent an LTEX identity or an asset path. Source inspection succeeds
for a valid null field while reporting unapplied_default_layers and runtime_ready=false.
Default selection/application still requires evidence before faithful rendering.

Texture lookup shares the existing safe byte-path normalizer with NIF materials.
It normalizes ASCII case/slashes and adds textures/ when the authored path is
relative to that directory. Original bytes remain in the report. Absolute exporter
paths, traversal, control bytes and empty components are rejected. No loose-file or
retail archive precedence policy is inferred from the available archive candidates.

## Bounds and caching

Default inspection limits are 128 unique texture records, 4,096 layer bindings,
256 unique assets, 64 candidates per asset and 256 MiB total decoded asset bytes.
Each selected LTEX/TXST stored and decoded body is limited to 64 KiB. These are
inspection budgets, not claims about retail content limits or measured peak memory.
The existing strict readers enforce tighter consumer limits before decompression
or archive extraction. Only one texture buffer is processed at a time; reports
retain metadata, and shared paths are read once per inspection.

The asset cache uses the existing profile/archive-digest/path/decoder identity and
committed manifest protocol. Reads still decode before publication/reuse checks;
this proves integrity, not accelerated startup. Publication verifies committed bytes.
NULL defaults do not create cache entries. A damaged copied image cache must fail
without changing the verified cache or retail installation.

## Independent evidence

The separate authored C++ terrain oracle now projects LTEX/TXST fields in addition
to WRLD/CELL/LAND. It consumes tagged Rust-decoded bodies, so compression and
canonical linking remain outside its independent scope. It is not an executed
xEdit application. Build it using [the exterior-field instructions](exterior-fields.md)
and compare --oracle-report with the same selected body cache. Changing one authored
texture-path byte in the oracle projection must fail.

The separate archive-member-oracle executable reads explicitly requested members
with pinned ba2, independently of dream_archive. Its request JSON is an array of
archive paths and original path_bytes. It emits source/output hashes and byte counts,
never image payloads. It shares project file guards/SHA helpers; plugin resolution,
mount selection, DDS pixels and shaders are outside this comparison.

~~~powershell
# After committing the implementation, use a fresh evidence directory.
.\target\release\fallout-evidence.exe --checkpoint 12 --run-directory .\local\terrain-12-verified --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
~~~

The runner compares six base/DLC fixtures, selected field projections and geometry,
cached/uncached reports, cold/warm texture receipts and every unique selected asset
against ba2. It damages a copied cache entry and verifies CLI rejection, runs the
workspace checks, and rehashes all original installation files. Public metadata is
in [terrain-textures.json](../reports/terrain-textures.json); bodies and images stay
under ignored local storage. Source and actual CLI/oracle executable identities are
bound in checkpoint-specific reports. Checkpoint 11 GPU evidence is retained, not rerun.

M1 remains open: original effective profiles, archive precedence and source-format
exceptions are unresolved. Defaults, inheritance, texture pixels/blending, water,
props, streaming, collision and gameplay are unfinished. No faithful scenario is accepted.
