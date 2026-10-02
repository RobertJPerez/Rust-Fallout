# 0001: Own compatibility and simulation; use replaceable presentation services

Date: 2026-10-02. Status: content ownership adopted; Bevy model presentation built and
exercised. Full interior and retail parity gates remain open. No custom renderer is being developed.

Use a standalone 64-bit Rust process. Keep game-specific decoding and canonical state
independent of the renderer. The current executable owns its reads and does not start,
inject into, or patch FalloutNV.exe. Native extender plugins are not runtime dependencies.

The bounded comparison covered three routes:

| Route | Evidence from this checkpoint | Decision |
| --- | --- | --- |
| Rust compatibility code with Bevy services | Pinned Bevy 0.19.1 builds on this host; two actual archived models captured through the GPU | Adopted for the model inspection view; full interior and shader parity remain open |
| Reuse focused Rust components | dream_archive and ba2 both built and compared on every local BSA member | dream_archive is the sole production archive backend |
| Reuse a broader replacement engine | ByroRedux README/provenance reviewed; implementation/license audit incomplete | No copied or linked code; do not let its advertised breadth set our parity status |
| External format/behavior oracles | esplugin built separately; xEdit FNV schema and identity mapping inspected | Keep oracle ownership and licensing separate from runtime implementation |

dream_archive at `a6ba4c9416e5c7bc211e7a3762420e5bf5c9c049` supports the NV BSA
variant without the native dependencies pulled in by ba2. Its strict decompressor
exposed two real malformed payloads. That failure behavior stays visible. We do not
silently fall back to the more permissive oracle in production.

The custom plugin reader implements bounded framing and retains unknown field bytes
during visits. It does not claim a schema for every record type. A persistent indexed
raw-record store and game-specific schema decoders are the next layer. Whole-record
override chains do not authorize arbitrary subfield merging.

Checkpoint 02 adds immutable on-demand record access and selected CELL/placement
schemas, plus an independent NIF container reader. It does not change the runtime
ownership decision or establish a persistent canonical world. Nifly remains a
separate GPL comparison tool; no native NIF library is linked into the Rust runtime.

Checkpoint 04 adds material decoding and a separate `fallout-preview` crate. Bevy
types appear only in the presentation adapter; the shared decoder owns no rendering
entities. The adapter uses Bevy's DDS reader, bounded upload checks and unlit diffuse
materials. A model capture is evidence for that loading path, not a gameplay scenario.

Use contiguous component storage for future hot simulation data and typed persistent
identities for saves; rendering entities will be transient views. Keep clocks, command
ordering, quests, inventory, and script state out of Bevy-specific components. Physics
will sit behind an explicit unit/query adapter. Rapier is still a candidate: no version
or collision parity has been validated in this checkpoint.

Workspace code forbids unsafe Rust. The archive dependency uses memory mapping;
Windows read handles deny concurrent writes/deletes and outlive those maps. Other
platforms need an equivalent source-lifetime policy before being advertised as supported.
See [contracts](../contracts.md), [source audit](../source-audit.md), and [source pins](../../sources.lock.json).
