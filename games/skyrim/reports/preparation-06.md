# Skyrim preparation checkpoint 06

This checkpoint adds two Skyrim asset-preparation commands on top of the already
pinned shared archive/path code. `nif-list` enumerates NIF member names from a
Skyrim BSA 105 index without decompressing assets; it returns at most 512 names,
records truncation and duplicate normalized paths, and surfaces hash-only or
invalid paths. `nif-header` resolves one normalized archive path, refuses duplicate
path identities, reads at most 64 MiB through the existing archive backend, hashes
the decoded bytes, and reports the common NIF line and fixed version tuple only.
Unknown tuples remain findings. Neither command changes installed data.

The read-only SE installation is Steam build `24914197`, runtime `1.7.104.0`.
The base `Skyrim - Meshes0.bsa` skeleton member is 24,207 bytes with 268 blocks.
Its NIF header is `20.2.0.7`, User Version 12, User Version 2 / SSE stream 100.
The four Creation archives present in this installation index 231 Fish, 266
Saints & Seducers, 65 Curios and 4 Survival Mode NIF paths. One mesh sampled from
each archive has the same SSE header tuple; all four path indexes report no
duplicate normalized names, hash-only entries or invalid asset paths. Three of
the four archive indexes return their 64-path cap and are explicitly truncated.

These observations establish header-format evidence for this SE installation and
the four installed Creations only. They do not decode NIF blocks, establish mesh,
material, skin, animation or Havok support, prove runtime consumption, or certify
the full paid Anniversary corpus. Earlier census evidence still reports that the
paid AE corpus is incomplete. Creation filenames are not treated as ownership
proof.

The reuse audit checked current NV main `4a4bb7a1e8f23cc378c45ce2519e6fdf542559e9`:
it now has NV-specific container, scene, geometry, material, skin and collision
work, while the shared Skyrim dependency remains frozen at verified checkpoint 45,
revision `9265ef63f714cdf01ff02028640a582be9639c7a`. Skyrim uses that shared
archive backend and path normalization, not the Fallout NIF decoder. Fallout 4
main remains `69f12091aa8b1b22530a8e203bdfbafc71c72a71`; its current worktree was
observed dirty and left untouched. It owns a FO4 3.9/game-2 PEX dialect, so this
project adds no PEX decoder or VM.

Validation passed with the workspace Cargo wrapper and one build job: 47 Rust
tests, formatting, warnings-denied Clippy, and a binary build. Synthetic BSA/NIF
fixtures cover path normalization, the SSE header tuple, visible unsupported
tuples and partial tuples, bounded result truncation and source-install report
protection. No gameplay or runtime compatibility scenario is accepted.

See the [bounded NIF preparation contract](../docs/nif-header-probe.md),
[reuse boundary](../docs/shared-integration.md),
[remaining Skyrim gates](../NEXT_STEPS.md), and machine record
[preparation-06.json](preparation-06.json) with its
[source manifest](source-manifest-06.json). The independent format reference and
license limits are listed in [sources.lock.json](../sources.lock.json).
