# Content and preparation contracts

## Identity and ownership

`FormKey` consists of a profile, normalized origin-plugin name, and a 24-bit local ID.
It does not persist a transient load-order selector. On disk, selectors below the master
count name a master; other selectors name the current plugin, matching the inspected
xEdit/esplugin behavior. Gun Runners' Arsenal contains noncanonical self selectors.
Multiple raw definitions resolving to one identity in the same plugin are rejected.
Master order must be explicit, unique, and dependency-valid.

SCRO links resolve in a second pass after all definition keys exist. Forward references
and ordinary record cycles are permitted; master dependency cycles are errors. NV's
engine-provided PlayerRef (`00000014`) is a separate, unimplemented runtime binding.
It is not fabricated as a plugin definition. Other missing IDs stay unresolved.
Deleted, persistent, and initially disabled flags are retained without inventing
instance behavior. Save/instance IDs and cross-profile save rejection are not implemented.

## Input boundaries

The NV reader accepts 24-byte record/group headers and the observed HEDR versions
1.32, 1.33, and 1.34. It checks every child extent, handles XXXX field sizes, retains
header bytes and decoded payloads, and records absolute header plus decoded field offsets.
Compressed field offsets are never mislabeled as absolute source positions.

Default budgets are 64 MiB per record, 4 GiB total decoded record bytes per plugin,
4,000,000 records, and 64 nested groups. Parsing walks a stack rather than recursing.
Zlib must finish at the declared output size and consume the entire stream. Diagnostic
checksum recovery is explicitly tainted and cannot enter strict content resolution.

BSA input is restricted to version 104. A header preflight caps entries/folders at
1,000,000 and each name table at 64 MiB before backend allocation. Backend parsing
checks index/data extents. Individual extraction is capped at 256 MiB. Reads currently
run synchronously in the headless tool; bounded background workers are still future work.

Archive lookup normalizes ASCII case and separators while retaining original bytes.
Traversal, absolute paths, empty components, and unsafe separators are rejected.
Non-ASCII bytes are preserved, not converted into a guessed Unicode identity. Plugin
filenames currently require ASCII; unsupported encodings produce an error. VFS entries
retain every candidate. Ambiguous paths have no implicit winner. Loose-file mounts,
invalidation rules, and production archive precedence still need implementation.

The census keeps metadata, not an in-memory copy of the whole game. The raw visitor
exposes unknown bytes to future consumers; those bytes are not yet persisted in a
canonical content store. Inspect previews are bounded to 64 bytes per field.

`RecordStore` now retains immutable source handles and a winning-record index for
on-demand reads. CELL/REFR/ACHR/ACRE inspection preserves group ancestry and original
transforms, and checks decoded base/enable-parent/door links against target kinds.
Its optional source-bound metadata cache persists raw headers and parent labels.
Every cached open hashes its sources and verifies the cache format/digest; canonical
identities and winners rebuild for the supplied order. Stored metadata does not
validate deferred record bodies. See [record index caching](record-index-cache.md).
See [world inspection](world-inspection.md) for the supported fields and limits.

The opt-in header index validates TES4/CELL metadata and defers other bodies.
Record/header bounds, IDs, master order and parent groups remain checked. Strict
on-demand decompression is mandatory; deferred bytes are never counted as validated
payloads. Census scope and decoded/deferred counts distinguish this path from a full
payload scan. Diagnostic recovery is refused by the deferred store constructor.

The NIF reader validates 20.2.0.7/user-11 outer container tables for the twelve
observed NV stream revisions, with exact block ranges and footer roots. It caps
input at 256 MiB, blocks/strings at 1,000,000 each, and string payloads at 16 MiB.
Six 20.0.0.4 containers remain unsupported. No block-internal reference, skin,
geometry, animation, material, or collision semantics are credited by this framing.

The separate `nif_scene` module decodes NiNode/BSFadeNode, NiTriShape/NiTriStrips,
and their two data types. It validates every supported block's full byte extent,
reference/string indices, finite source floats, and topology indices. Scene child
cycles and repeated/multiple parents fail; unknown child types remain explicit.
Traversal is iterative and includes disconnected components. Footer reachability is
reported separately from the existence of a transform. Default scene limits are
64 MiB input, 100,000 blocks and 128 MiB aggregate payload-array element storage;
the container and graph tables have their own count bounds.

Stored strip counts include degenerate connector steps. Expanded triangles omit
repeated-index connectors while preserving winding and restarting parity for each
strip. Raw indices and source counts remain available. Transforms retain source
coordinates; no game-to-renderer axis or unit conversion is applied. Every scene
still reports `runtime_ready=false`: reading triangles does not evaluate visibility,
materials, skinning, controllers, collision, or gameplay. See [scene decoding](nif-scenes.md).

Nine material/texture payload types now preserve raw properties, paths, version gates
and texture links. Derived path storage shares the scene allocation budget. External
paths are limited to 4,096 bytes and must remain safe relative archive paths. The
texture probe deduplicates normalized paths, retains all usages and rejects ambiguous
mounts. It validates cache destinations before starting. See [material decoding](nif-materials.md).

Presentation stays in `fallout-preview`: Bevy-specific meshes, images, axis conversion
and camera controls do not enter `fallout-data`. The current view supports BC1/2/3
diffuse DDS with bounded dimensions and exact mip payload sizes. It reports skipped
features and never changes the decoder's `runtime_ready` or gameplay acceptance.

Authored untextured NoLighting type 33 and supported material-only bindings are
distinct from unresolved diffuse bindings. The latter remain reported magenta.
Raw alpha/culling/depth fields drive a separate extended-material adapter; embedded
WGSL performs alpha testing before fixed-state blending. Synthetic GPU measurements
are recorded separately from retail-derived scene captures and acceptance. They do
not certify NoLighting falloff, emittance, lighting or stencil-buffer operations.

The separate nif_collision module preserves sixteen NV collision payload kinds
without choosing physics units or a backend. Typed attachment/body/shape/constraint
links are checked separately. Shape sharing is legal; shape cycles are not.
Unsupported links retain whether their expected type family was verified.
Packed triangles retain winding, degenerates and welding; MOPP remains opaque.
Source float fields require finite values, while alignment words remain raw bits.
Full-block consumption and payload-array budgets use the same bounded cursor as
scene decoding. Original compressed vertex words are retained without an assumed
half-float conversion. Every result remains physics_ready=false. See
[collision decoding](nif-collisions.md) for evidence, limits and remaining work.

Source reference math lives in the engine-independent `coordinates` module; Bevy
conversion and ECS views remain in presentation. Static placement uses a provisional
clockwise X/Y/Z convention, preserving source values and subtracting an f64 source
origin before narrowing to relative f32. Actor rotations need a separate adapter.
Retail measurements still gate this convention. Models and texture/sampler pairs
are shared across references; interior preparation caps 10,000 references, 2,048
models, 256 MiB estimated prepared geometry, 4,096 texture/sampler pairs and 256 MiB
encoded DDS storage. Per-decoder limits also apply. Preparation is synchronous.

BSXFlags name/value fields now decode with exact payload consumption; unknown flag
bits remain intact. Presentation omits named EditorMarker/VisibilityEditorMarker
mesh branches only when their decoded root declares BSX bit 5. This does not hide
standalone marker models, evaluate collision flags or change canonical source data.

## Jobs and publication

The dry-run planner hashes source content and produces deterministic preparation jobs.
Job identities include profile, operation, transform revision, source digest, and ordered
master digests. Reordering directory enumeration or moving the installation does not
change identities. A changed master invalidates dependent plugin jobs. The planner
currently schedules plugin/archive indexing only; it lists loose inputs and all missing
conversion stages. `runtime_ready` is always false at this stage.

Single-asset cache keys contain profile, source archive SHA-256, normalized member path,
and decoder revision. Publication stages files in the cache directory, syncs them,
publishes the blob without replacing a winner, then publishes its manifest. The manifest
is the commit marker. Resume verifies orphaned blobs; reuse verifies both identity and
payload digest; corruption is an error. The CLI currently decodes before checking reuse,
so this is correctness groundwork, not a measured startup optimization.

This rebuildable cache does not promise save-grade power-loss durability. Full job
cancellation, conversion journals, graph scheduling, and transitive rule invalidation
are unfinished R3-04 work. Multi-pass inspection assumes source files remain unchanged
between passes; individual Windows source handles protect their own open lifetimes.

Metadata cache publication now has actual process-termination tests at blob staging,
blob publication and manifest staging. A missing commit marker triggers a rebuild
before orphan verification; corrupt committed entries fail. These tests verify this
rebuildable publication path, not save-grade durability or complete job cancellation.

## Evidence boundaries

`decoded` means recognizable bytes, not behavior. CTDA IDs are not SCDA opcodes. Script
headers and references are inventoried without executing code. No behavior-critical
unknown may be promoted to `accepted` by changing a README checkbox. There are no
accepted scenarios in this checkpoint. Native DLL presence, advertised upstream features,
and a successful archive read are not compatibility evidence.
