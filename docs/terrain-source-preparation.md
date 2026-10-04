# Exact exterior texture source preparation

The existing terrain diagnostic has an opt-in bounded texture source job path:

```powershell
fallout --output local/goodsprings-source-new.json terrain --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --editor-id Goodsprings --inspect-textures --prepare-textures --texture-cache local/goodsprings-source-cache
```

The caller selects an explicit plugin order. This mode requires exactly one
winning present LAND belonging to the winning exterior CELL and the same world
group. Multiple, absent or deleted LAND selections are an explicit unsupported
branch. Existing invocations without `--prepare-textures` retain their native
texture report and synchronous archive reader. The new flag conflicts with the
tagged `--body-cache` oracle path.

`terrain::preparation::TextureSourcePlan::load` constructs a sealed immutable
plan from protected winning source records. It reuses the existing CELL/WRLD,
LAND, LTEX and TXST decoders and texture dependency collector. The receipt keeps
ordered plugin hashes, the exact root, physical record headers and parent group
labels, strictly decoded body hashes, LAND layer/alpha order and offsets, raw
LTEX/TXST links, all authored TX00..TX05 paths and every archive candidate.
`decoded_offset` follows the existing inspector convention: it points to the
six-byte subrecord header; field data starts six bytes later.

Empty and absent path fields remain distinct. Null layers, missing/wrong-kind/
deleted links and archive collisions remain visible. No default material,
worldspace inheritance, loose-file policy or archive precedence is applied.
Only one-candidate paths become jobs. Each selected BSA is opened under the
existing source protection and hashed, and the real member index, raw path and
declared decoded length are verified before admission. Plan identity hashes the
exact source evidence, including candidates and selected archive identities.

`TexturePreparation` uses the existing `ResourceJobs` worker pool, source cache
identity and reservation ownership. `poll` consumes at most eight completions
and admits at most eight requests. Completed raw member payloads remain attached
to their source/byte reservation while hashes/cache receipts are collected, then
are dropped. No copied resource payload escapes that reservation. Selected
completion metadata stays private until `take_ready` yields the entire batch.

`Ready::publish_for` checks the caller's complete terrain source binding and
commits only within the current owner epoch. Replacement advances that epoch
even for identical source bytes. Cancellation, retry, source replacement and
owner drop revoke queued/running/completed work and already taken Ready values.
Failure clears private staged results; it requires a fresh retry and cannot
publish a partial selected batch. Valid immutable source cache entries may be
reused after retry. A partial cache entry cannot satisfy the member's declared
source length even if its own manifest hash is internally consistent.

The new CLI output includes `texture_preparation`: immutable plan metadata plus
complete selected texture byte/hash/cache receipts. `all_requested_texture_sources_ready`
covers only the selected unambiguous members. `all_authored_texture_sources_resolved`
also requires no unresolved source links, missing/ambiguous assets or unapplied
null layers. Both are source diagnostics. `runtime_ready` remains false.
Already published receipts are historical diagnostics; they are not live world
state. The preparation owner must remain alive until Ready publication.

Ceilings are lowerable, never raiseable: one million winning definitions,
256 plugin sources, 256 parent worlds, 128 texture records, 4,096 layers,
256 distinct asset paths, 4,096 candidates, eight selected archives, 1 MiB
per selected plugin body, 8 MiB aggregate decoded plugin bodies, 262,144
physical field sites, 4,096 bytes per lookup/candidate path, 64 MiB total
declared selected texture bytes and 32 MiB conservative logical metadata.
Reads and typed collection/copies are admitted before allocation. Identity
serialization streams into a hash with a separate 512 MiB write ceiling.
These bounds exclude shared plugin/archive indices, archive mappings, allocator
overhead and downstream report/image allocations. They do not measure process
peak memory. Planning/indexing/source hashing is synchronous, and one admitted
BSA decode is opaque internally; existing checks before and after decode prevent
stale publication without claiming fine-grained interruption inside that reader.

Authored checks cover cold/warm/changed-cohort cache identity, layer/alpha sites,
null/missing/deleted/wrong-kind/absent links, collisions, all lowerable limits,
forensic source rejection, wrong publication binding, identical replacement,
queued/running/completed stale work, cancel/drop, reservation/source pin release
and partial-cache rejection followed by retry. Private installed evidence uses
explicit base-only Goodsprings and independent physical plugin/BSA comparisons.
Source parity does not establish DDS/image interpretation, shader/blend behavior,
grass/LOD/water, collision, gameplay streaming, input or retail acceptance.
