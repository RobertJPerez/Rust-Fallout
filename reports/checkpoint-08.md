# Implementation checkpoint 08: reusable plugin indexes

The Rust cell inspector can now reuse persisted metadata from the ten official NV
plugins. Every cached open verifies full source hashes and encoded index digests.
Strict on-demand record access and supplied-order override resolution remain active.
The game is still an inspection runtime; this checkpoint does not close M1.

| Check | Result |
| --- | --- |
| Workspace | Formatting, 81 tests and Clippy with warnings denied pass |
| Builds | Windows MSVC release CLI and Rust evidence runner build |
| Persisted metadata | 629,788 record definitions in 30,289,643 encoded bytes |
| Cold/warm verification | Ten entries built; all ten reused on the next open |
| Selected cell | 435 references, 222 bases, zero selected integrity/link failures |
| Field comparison | Every selected serialized field matches the uncached path |
| Deferred bodies | 585,196 remain explicitly unvalidated until access |
| Source hashing | 311,381,078 plugin bytes hashed on each cached open |
| Process termination | Children killed at three publication stages; verified recovery |
| Original installation | Fresh 464-file hashes match the original baseline |
| Accepted gameplay | None; physics, scripts and movement remain open |

Cache identities include profile, source bytes, plugin name, decoder version and
framing budgets. They do not depend on the installation directory. Raw FormIDs
and parent labels are persisted, while canonical identities and winners rebuild.
Reordered plugins, moved/deleted overrides and changed masters preserve their
correct current lookup behavior. Unchanged child metadata can be reused when a
master changes because the live view is rebound rather than persisted.

The application metadata format bounds encoded bytes, summary bytes, record counts,
editor IDs and retained definition storage before allocating arrays. It checks
source extents, option tags, duplicate/null IDs, summary consistency and exact
consumption. Committed corruption is an error. A missing manifest leaves a blob
unavailable; rebuilding establishes the expected bytes before orphan verification.

Eleven authored integration tests exercise actual cached record-store access,
including changed sources of identical length, reordered/moved/deleted winners,
master rebinding, source relocation, strict limits, bad cache data, malformed
metadata and deferred checksum failures. Metadata truncation and 256 deterministic
mutations supplement the existing parser tests. A unit test runs dedicated child
processes and kills them after blob staging, blob publication and marker staging.
The intentionally ignored worker test is executed only by that parent test.
Its synchronization hooks are excluded from the runtime build.

The evidence runner executes uncached, cold-cache and warm-cache inspections using
the release CLI, checks exact fields and receipts, hashes sources/artifacts and
rehashes the complete original installation. Raw cell/cache data stay local.
[record-index-cache.json](record-index-cache.json) records actual wall-clock samples;
they include complete CLI inspection and are not a sustained startup benchmark.
The cache still hashes all source bytes, rebuilds winners and runs synchronously.

The preflight selected fields also equal checkpoint 05's strict cell report.
Checkpoint 07 collision comparisons and checkpoint 06 GPU captures remain historical
evidence; they were not rerun in checkpoint 08. Earlier immutable verification and
source snapshots remain available.

Archive/loose precedence, an effective measured retail profile, known source format
exceptions, remaining typed fields and canonical content storage are open. Job
queues, cancellation, journals, staging cleanup and save-grade persistence remain
unfinished. See [NEXT_STEPS.md](../NEXT_STEPS.md),
[record index caching](../docs/record-index-cache.md),
[verification.json](verification.json) and [source-snapshot.json](source-snapshot.json).
