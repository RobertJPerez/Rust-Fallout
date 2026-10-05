# Reuse and ownership

The Skyrim work reads New Vegas's current committed source and handoffs as an
interface map; it does not edit or build from New Vegas working trees. On
2026-10-05 the NV main checkout was observed clean at
`a909a35bf0cc3fd3e05ceab34339116ce8da9553`. Its parent contains the latest
committed Rust source, `486355d67407080d0e2c2483555518c0fb798670`; the current
commit publishes a development receipt. Checkpoint 45 at
`9265ef63f714cdf01ff02028640a582be9639c7a` remains the latest accepted shared
data proof. The v4 receipt maps 31 reviewed development changes but explicitly
retains an open workspace gate: its scoped checks come from distinct source
revisions, the latest attempted composed Clippy failed, the current CLI/preview
build is pending, and M1 remains unmet. The receipt does not promote those
changes to main or accept original gameplay. The Skyrim dependency remains pinned
to checkpoint-45 shared data; later NV work does not silently move that pin.
Keep NV user, stream and block schemas out of Skyrim-specific adapters.

| Concern | Existing owner and evidence | Skyrim work |
| --- | --- | --- |
| Record/group framing, XXXX, zlib and source hashing | Pinned `fallout-data::plugin` and `fallout-data::baseline` | Skyrim TES4 metadata and field adapters call the shared readers; no second ESM decoder |
| BSA decoding and compression | Same pinned `dream_archive` backend used by NV | A narrow BSA 105 admission adapter; the NV 104 guard is unchanged |
| Asset path rules | Pinned `fallout-data::vfs::AssetPath` | Reused for script bindings and model paths; Skyrim adapters add only their schema-owned `meshes/` or `textures/` root before exact loose/BSA name lookup |
| NIF container and payload work | Pinned `fallout-data::nif` indexes bounded `20.2.0.7`/user-11 NV containers; NV main adds scene, mesh, material, skin and collision decoders for observed NV tuples | Skyrim reuses BSA 105 member loading and adds its own exact-tuple user-12/stream-100 outer-table adapter; all block payloads remain opaque, and no Fallout payload parser was copied or run on Skyrim data |
| Skyrim static model references | TES5-specific `STAT` schema and `MODL`/`MNAM` source fields | A Skyrim-owned adapter retains `MODT`/`MODS` hashes, splits four `MNAM` slots and reports exact plus `meshes/`-rooted candidates without selecting an archive or plugin winner |
| Skyrim texture-set slots | TES5-specific `TXST` `TX00`–`TX07` fields | A Skyrim-owned adapter maps texture-root-relative slots to Data-root `textures/` paths and reuses the same names-only loose/BSA matcher; decal flags, material meanings and winners remain unresolved |
| Actor, placement and leveled source services | Current NV data/runtime code contains source catalogues, template/list graphs, actor placement and explicit unresolved dependencies | Skyrim-specific `SPLO`, `TPLT`, `LVSP`/`LVLN`, and `ACHR NAME` adapter preserves disk bytes and does not import Fallout record rules or claim runtime inheritance/selection |
| Player movement source profile | NV world and actor state is not a Skyrim movement oracle | Skyrim `NPC_` `RNAM` → `RACE` → `MOVT` source graph retains movement fields, exact float bits and physical candidates without choosing overrides, converting units or claiming locomotion behavior |
| Runtime state, events and persistence | NV runtime and active runtime lane | Deferred until the coordinator reviews a Skyrim identity boundary and shared consumer APIs |
| Rendering, world streaming, animation and physics | NV world/assets/presentation/physics lanes | Future SSE/Havok adapters; NV source versions and behavior are not treated as Skyrim support |
| Papyrus PEX definitions and binding | Fallout 4 working tree has an uncommitted public `pex` module pinned to PEX 3.9/game 2, plus VMAD declaration binding and static call/branch inventories | Skyrim census now records only the eight-byte PEX dialect prefix; keep all PEX bodies opaque until a Skyrim dialect is independently tested and a shared API is agreed |

`NvArchive` is intentionally not used for Skyrim: its admission guard requires
BSA 104. The shared lower-level archive backend handles BSA 105 LZ4. The thin
Skyrim adapter validates the v105 folder records and allocation limits before
opening that backend. It never relaxes the NV guard.

The pinned NIF indexer likewise rejects Skyrim's SSE tuple by design: it admits
NV user-11 streams 14 through 34, while installed Skyrim SE mesh headers are
user-12/stream-100. Do not rewrite a Skyrim tuple to enter the NV parser or
change the active NV guard. A bounded Skyrim-owned outer-table adapter now
handles the installed tuple and keeps each block opaque; a shared versioned API
can be considered when the shared dependency is deliberately reviewed. Skyrim's
20.2.0.7 geometry inheritance changes still require distinct payload semantics.
See
[`nif-header-probe.md`](nif-header-probe.md) for the bounded handoff.

The shared `ProfileId` enum has no Skyrim value. No Skyrim identity is stored as
Fallout or crossover. Source provenance retains the plugin and file-local ID; a
record owned by an ESL can be keyed to that source file when its HEDR range is
valid. The Skyrim-owned `map-profile` adapter computes separate full/light slots
from a caller-supplied, order-checked plugin sequence without extending
`ProfileId`. The standard Windows game-profile paths were empty, so no order was
available to test against. Cross-file `FE` references in
plugin-file context and winning overrides still require verified profile evidence.
File presence alone does not establish Creation ownership or a complete paid
Anniversary installation. The TES4 light flag determines slot class; `.esl`
filename suffixes are inventoried separately.

The current measured installation is Steam build 24914197 / executable 1.7.104.0:
the five base masters, the resource-pack ESL, and four included Creation plugins
are present. This source profile does not stand in for the full paid AE content
set. Its executable version and plugin/archive inventory are recorded separately.

Fallout 4 main was observed at `69f12091aa8b1b22530a8e203bdfbafc71c72a71` on
2026-10-05, with extensive active uncommitted changes; its working tree was left
untouched. The current PEX module rejects every tuple other than 3.9/game 2, and
its call operand constraints, dispatch, native behavior and runtime are not
Skyrim evidence. The newly exposed Skyrim census prefix is only a dialect
inventory, not a PEX decoder or a claim that the FO4 module accepts Skyrim. The
Skyrim VMAD binding exports, script hashes, exact property bits and unknowns are
ready as inputs after a Skyrim-specific dialect passes independent tests and
the owners agree on the API boundary.

To update the shared dependency, inspect the NV coordinator's verified checkpoint
and current handoff, deliberately change the exact Git revision, update
`Cargo.lock`, run Skyrim tests/lints and compare a fresh retail census. Do not use
an unpinned path dependency against active working trees. Original game files
remain read-only; detailed corpus output remains under ignored `local/`.
