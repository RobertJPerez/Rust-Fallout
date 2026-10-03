# Source-bound plugin index cache

The headless cell inspector can now reuse persisted plugin metadata. Every cached
open hashes each plugin through a retained read-only source handle, then verifies
the cache manifest, encoded length, payload digest and bounded metadata format.
Record bodies still go through the ordinary strict on-demand reader.

Checkpoint 08's first real-data run stores 629,788 record definitions from all ten official
plugins in 30,289,643 encoded bytes. The second run reuses all ten entries.
Both reproduce all selected Doc Mitchell fields: 435 references, 222 bases and
zero selected integrity/link failures. Their selected fields also match the earlier
checkpoint-05 uncached report. This validates metadata reuse, not every game body:
585,196 unrelated bodies remain explicitly deferred.

## Use it

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli
New-Item -ItemType Directory -Force .\local\record-index-cache | Out-Null
.\target\release\fallout.exe cell --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order .\profiles\nv-inspection-order.json --editor-id GSDocMitchellHouse --defer-unread-payloads --index-cache .\local\record-index-cache --output .\local\cell-cached-new.json
~~~

Choose a fresh report filename for each run. The cache directory must already exist
outside the entire installation. Cached indexing requires strict deferred reads;
diagnostic checksum recovery cannot produce cache entries. Omitting --index-cache
keeps the existing uncached path. Presentation does not use this cache yet.

The report's optional index_cache field lists each plugin's source SHA-256, artifact
key, metadata size/digest and build/reuse outcome. It also identifies the ordered
source set and reports how many bytes were hashed. Uncached reports omit this field,
preserving the earlier serialized output.

## Identity and dependencies

Each artifact key includes the NV profile, exact plugin filename bytes, full source
SHA-256, metadata decoder version and framing budgets. It excludes the installation
directory, allowing byte-identical sources to move. A different filename spelling,
source byte, decoder revision or budget gives another key. Timestamp and file length
alone never qualify an entry for reuse.

Only raw source FormIDs, record headers, editor IDs, parent labels and index census
metadata are persisted. Canonical identities and override winners are rebuilt for
the supplied order on every open. Changing that order changes the ordered-source
identity and winning definitions. A changed master invalidates its own index and
rebinds the live view; an unchanged child's raw metadata remains reusable. No linked
asset or gameplay output is cached under an outdated master identity.

The parser preserves deleted winners and winning parent membership. Reuse cannot
resurrect an earlier reference or leave a moved override in its old cell. Missing or
misordered masters, duplicate definitions and canonical identity collisions still
fail through the existing resolver.

## Format and limits

FNVHIDX version 2 is an application cache format, not a Bethesda format. Its
20-byte prefix identifies the format, version, record count and summary size.
The bounded JSON summary retains the selected census and script-reference metadata.
Record entries store a 32-byte header projection, four tagged parent values (topic, world, cell and child group) and an
optional editor ID. Raw parent labels remain local to the source plugin.
Checkpoint 23 adds the topic value and changes the transform identity to v2.
Older v1 bytes are rejected by the new decoder; their existing artifacts stay
separate. Current byte counts and fresh cell/membership comparisons are in
[dialogue membership](dialogue-membership.md).

Maximum encoded input is 128 MiB per plugin, summary input is 1 MiB and an editor
ID is at most 4,096 bytes. Before allocating the record array, the decoder checks
its count against available bytes, the caller's record limit and 256 MiB of retained
definition/editor-ID element storage. It also checks ordered extents against source
length, duplicate/null IDs, canonical option tags, supported child-group labels,
summary consistency and exact payload consumption. Temporary sets, JSON allocations,
container metadata and allocator overhead are outside that element-storage estimate.
These are parsing limits, not a measured peak-memory guarantee.

New metadata-format or index-semantic changes must bump the decoder version in the
artifact identity. Framing-limit changes already produce distinct keys. Cache
validation does not authenticate artifacts supplied by an external party.

## Publication and recovery

The existing cache publishes through synced staging files and refuses to replace
an existing winner. The JSON manifest is the commit marker. No marker means the
artifact is unavailable: a builder reconstructs its expected metadata before
verifying and committing a matching orphan blob. A committed corrupt/mismatched
entry produces an error. It is not silently accepted or replaced.

The test executable launches dedicated children and terminates them after blob
staging, blob publication and marker staging. Each case leaves no committed partial
artifact; publication resumes, validates the original bytes and leaves source
fixtures unchanged. Test synchronization hooks compile only into the unit-test
executable. Stray staging files are ignored; automatic orphan-stage cleanup is open.
This rebuildable cache is not the save system or a complete transformation journal.

Eleven authored integration tests cover cold/warm equivalence, same-length source
changes, changed masters, reordered/moved/deleted winners, relocation, stricter
limits, bad source/master/cache data, orphan recovery, malformed metadata and deferred
checksum rejection. Metadata tests reject every truncated fixture prefix and exercise
256 deterministic byte mutations. No retail plugin bytes are distributed as fixtures.

## Reproduce the checkpoint

~~~powershell
# Commit the tested implementation and choose a fresh local run directory.
.\target\release\fallout-evidence.exe --checkpoint 8 --run-directory .\local\index-08-verified --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
~~~

The Rust runner executes workspace checks, uncached/cold/warm cell inspections and
a full installation baseline. It requires exact selected-field equality after
removing only the cache receipt. It checks all cold entries were built and all warm
entries reused, then verifies source/artifact hashes before publishing metadata.
The [record-index report](../reports/record-index-cache.json) records actual command
timings and hashes. Each phase has one sequential wall-clock sample, including
archive indexing and dependency reads; this is not a sustained benchmark or a
startup performance guarantee.

This checkpoint does not close M1. Effective retail profiles, archive/loose
precedence, known format exceptions and remaining typed content are still open.
Physics, scripts, animation and gameplay remain unfinished.
