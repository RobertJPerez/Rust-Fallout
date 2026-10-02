# Implementation checkpoint 07: authored collision payloads

The standalone Rust content reader now decodes NV's authored collision data
independently of the renderer. It preserves rigid-body fields, primitive shapes,
convex vertices/planes, wrapper graphs and packed triangles in their original
coordinates. The inspection camera still moves through walls; physics has not
been activated.

| Check | Result |
| --- | --- |
| Workspace checks | Formatting, 68 tests and Clippy with warnings denied pass |
| Builds | Rust CLI/evidence tooling and separate MSVC nifly oracle build |
| House comparison | All 207 model candidates match; 731 supported collision blocks |
| Older-stream samples | All 11 match; three supported collision blocks |
| Total supported projection | 734 collision blocks; 15 of 16 implemented names sampled |
| Packed geometry in house | 6,622 vertices and 2,810 triangles |
| Convex vertices in house | 964 |
| Authored MOPP bytes in house | 23,760; retained opaque |
| Unsupported house payloads | 19 constraints and 18 blend controllers |
| Shape graphs | Shared children allowed; cycles rejected through an iterative walk |
| Malformed inputs | Bounds, references, floats, booleans, triangle indices and budgets checked |
| Original installation | Fresh hashes match the original 464-file baseline |
| Gameplay acceptance | No accepted scenarios; M1 and M2 remain open |

The independent comparison uses pinned nifly factories without PrepareData.
It compares every supported projected field exactly, including stored binary32
bits, source/block SHA-256, offsets, lengths, flags, references, topology and
alignment words. The separate Windows oracle hashes source bytes through CNG;
the original Rust decoder uses SHA-256 without a native dependency. This evidence
verifies parsing against a second implementation, not original Havok behavior.

The parser distinguishes active bhkRigidBodyT transforms from ordinary body
fields, retains duplicate scale/radius values, and keeps degenerate triangles and
welding words. It performs no guessed axis/unit conversion or visual-mesh
replacement. MOPP is not interpreted. Compressed packed vertices retain their source
words; that branch has authored tests only because pinned nifly does not decode it
correctly. bhkPCollisionObject also has authored tests without a retail sample.
Eight older-stream samples have an empty supported collision projection.

Twelve new authored integration tests include shared shapes, disconnected cycles,
a 10,000-wrapper chain, truncated/surplus payloads, nonfinite meaningful fields,
raw alignment NaN bits, wrong reference roles, resource budgets and 512 deterministic
mutations. Two CLI unit tests check float-bit projection and rejection of nine
deliberate comparison mismatches. Existing parser, cache and presentation tests pass.

The Rust evidence runner executes the workspace checks, both independent model
comparisons and a full installation baseline. It binds source/configuration files
to the implementation commit and confirms executable hashes remain unchanged.
Raw retail-derived reports remain local; committed summaries contain counts and
identities. Checkpoint 06's presentation/GPU results remain historical evidence:
the renderer is unchanged and GPU captures were not rerun in checkpoint 07.

Physics queries, constraints, collision margins, retail axes/units, body/placement
composition and character movement remain open. Archive precedence, known format
defects, scripts, saves and other-game support also remain unfinished. Continue with
[NEXT_STEPS.md](../NEXT_STEPS.md), retaining the brief's dependency gates.

See [collision decoding](../docs/nif-collisions.md) for commands and limits,
[nif-collisions.json](nif-collisions.json) for per-sample counts,
[verification.json](verification.json) and
[source-snapshot.json](source-snapshot.json) for source/binary identities.
Checkpoint 06's [verification](checkpoint-06-verification.json) and
[source snapshot](checkpoint-06-source-snapshot.json) are preserved.
