# Reuse and integration contract

NV baseline: `9265ef63f714cdf01ff02028640a582be9639c7a`, verified checkpoint 45.
This revision was selected from its checkpoint report, rather than an active
worker head. The NV team continues independently; this project owns no NV lane.

The read-only NV checkout was observed at `5a89c33bee93ccf6f6ff0cb489881ff9953a6c5b`
on 2026-10-05 01:10 UTC and was clean. Its latest Rust source revision is
`486355d67407080d0e2c2483555518c0fb798670`; the later commit adds only
`NEXT_STEPS.md` and a dependency handoff receipt. A disposable copy of this
Fallout 4 workspace was tested against that exact committed Rust source. All 86
all-target tests passed with one compiler job. This checks shared API/build
compatibility only; it does not establish gameplay, rendering, save or native
API behavior. The current handoff reports passing receipt/protocol tests while
the actual GPU consumer remains open. See
[`reports/nv-compatibility-checkpoint.json`](../reports/nv-compatibility-checkpoint.json).

Fallout 4 remains pinned to verified checkpoint 45
(`9265ef63f714cdf01ff02028640a582be9639c7a`). It uses the shared plugin framing,
path validation and archive contracts, while Fallout 4-specific profile,
material, workshop and Papyrus content stays in this workspace. No NV runtime
lane is duplicated, and the Fallout 4 dependency does not follow branch movement.

| Area | Decision |
| --- | --- |
| TES4/GRUP framing, XXXX and zlib | Call existing `fallout_data::plugin` visitors directly. Per-game semantics remain separate. |
| Asset path byte handling | Reuse `fallout_data::vfs::AssetPath`, including traversal rejection. |
| Archive backend | Enable BA2 on the exact `dream_archive` revision already used by NV. No second production archive implementation. |
| Stable identities | Re-export shared `FormKey` / `ProfileId`; FO4 small-plugin IDs are currently preserved as raw observations, without the NV resolver or an ESL slot assignment. |
| Simulation, saves, streaming, renderer | Owned by NV; reuse after its contracts are ready. No duplicate implementation here. |
| Papyrus | FO4-specific structural decoder and static call/branch/opcode inventory, with pinned compiler-authored call-signature, branch, struct and array fixtures; no ObScript opcode reuse or VM execution claims. |
| Materials | FO4 BGSM/BGEM fields and direct-BA2 candidate provenance are inspected here; 24 sampled DDS top mips were cross-checked through ImageMagick and DirectXTex within one 8-bit code value. Game-renderer, shader and color-space behavior remain open. |
| Workshop and scrap records | FO4 COBJ, CMPO, MISC and GLOB fields are structurally decoded; pinned-schema FormLink slots carry raw bit-pattern evidence and CTDA slots receive type hints, while FormID resolution, eligibility, settlement placement and transactions are not implemented. |
| BA2 precedence | Physical archive/member provenance only. Ambiguous names fail lookup. No invented global winner order. |
| Localization tables | An offline FO4 census extracts direct-BA2 string tables to ignored `local/` and compares raw key offsets/value bytes through pinned Mutagen code. It does not decode text, choose locale, apply loose/VFS precedence or add a runtime archive resolver. |
| Recursive install inventory | An offline fingerprint pass covers every physical file below the retail `Data` directory and compares direct files to the frozen census. Details remain under ignored `local/`; it does not observe VFS deployment, active profiles, or effective asset winners. |
| Game configuration | An offline FO4 tool resolves the Windows Documents known folder and preserves only allow-listed INI files under ignored `local/`. It reports no setting values and does not infer which profile or settings the game loaded. |
| Profile state | A metadata-only audit hashes known global/Vortex profile and deployment files without publishing names or contents. It does not identify a manager's active profile or VFS. |

`Ba2` adds count admission before the backend allocates its index. Decompressed
members are capped before opening a stream. DX10 headers/mip ranges remain in the
backend's typed representation. The library reconstructs DDS when reading a texture;
metadata census alone does not verify every texture's decoded bytes or pixels.

`pex::File` borrows the complete immutable source bytes. Functions/instructions
retain byte offsets, typed operands and exact float/bool bits. Struct members,
variables, initial values, properties, backing-variable names, accessors and state
definitions are now typed trees. Accessors and states refer to the owning object's
function vector by index/range. Debug details still remain raw ranges.
Node/file/vararg budgets bound work and allocation. Only FO4 3.9/game 2 is admitted.
FO76/Starfield opcodes fail. Branches outside the function fail; the static
inventory maps in-range branches to instruction indices and retains a branch to
the one-past-end boundary without assigning continuation semantics.
Object lengths accept two explicit compiler conventions: retail counts the size
field itself (observed in installed PEX), while pinned Caprica counts body bytes.
Both are bounded and checked against actual consumed bytes; other sizes fail.

Decoding validates framing, string references and selected structural invariants.
The executable inventory reports exact-name static-call declaration candidates and
branch destinations, but does not prove operand type rules, runtime
inheritance/dispatch, call resolution, numeric behavior, scheduling, alias filling,
native effects or serialization. The native list represents declarations, not
implemented APIs or resolved calls.

`link::Catalog` records every physical class definition and its source hash/path.
It performs ASCII case-insensitive name binding and bounded derived-first property
and default-state method lookup through unique parents. Duplicate class names,
duplicate local declarations, missing parents, cycles and unsupported identifiers
have distinct results. Matching a declaration does not apply VMAD values, remove
scripts, select an archive winner or dispatch a runtime event.

Reports count physical records and archived scripts, including overrides/duplicate
definitions. They must not be labeled the active game's effective content.
The physical plugin master graph preserves TES4-declared edges and resolves only
unique filenames in the frozen census. It is not an activation order or an
effective-plugin resolver. Raw small-master record IDs remain separate evidence.
The workshop catalog uses the pinned FO4 schema, preserves per-record versions,
raw FormIDs, GLOB value bytes and exact CTDA/CIS bytes, and independently matches
COBJ, CMPO, MISC and GLOB field structure against the pinned Mutagen reader.
Typed COBJ/CMPO/MISC FormLink slots also retain exact raw values and offsets;
37,699 non-null local-ID candidates matched the independent structural reader in
the frozen proof. Numeric ID comparisons remain observations only; GLOB numeric
bits are not interpreted. It does not resolve runtime identity or ingredient
names, evaluate conditions, choose an override or perform workshop actions. See
docs/workshop-recipes.md.
The original census retains its VMAD header-only scope. The separate VMAD reader
now decodes bounded attachments/properties/fragments and keeps raw form IDs,
unknown flag bytes, string bytes and float bits. The independent comparison
captures IDs before the reference reader's master-slot normalization. See
docs/vmad.md for unsupported types and disputed packed-field semantics.
Record versions are inventoried instead of selecting all formats from the
executable's version label.

The standalone CLI writes only when explicitly extracting evidence for an offline
oracle. All research binaries and extracts stay under ignored `local/`. No oracle
is a production dependency. Source files must remain immutable during inspection;
the census is not a transactional snapshot of a concurrently updated install.
