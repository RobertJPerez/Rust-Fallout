# Implementation checkpoint 05: strict selected reads and a placed interior

Doc Mitchell's real house now renders through the Rust content pipeline and Bevy
adapter. Two GPU views show the cabinet room and bedroom, using actual plugin
placements and archived model/texture bytes. Repeated references share geometry,
materials and texture/sampler storage. The source has been uploaded to
[RobertJPerez/Rust-Fallout](https://github.com/RobertJPerez/Rust-Fallout).

| Check | Result |
| --- | --- |
| Workspace checks | Formatting, 52 tests and Clippy with warnings denied passed |
| Builds | Release CLI, debug preview and separate MSVC raw nifly oracle passed |
| Strict selected cell reads | 435 references, 222 base records, zero integrity/link failures |
| Deferred index scope | 585,196 bodies deferred at indexing; unread payloads remain unvalidated |
| Full/deferred source comparison | All ten selected field groups match exactly |
| Deferred corruption | Synthetic corrupt body rejected on access; truncation/count checks retained |
| Static presentation | 400 references, 203 shared models, 722 mesh instances |
| Shared prepared geometry | 96,394 vertices, 84,378 triangles before instancing |
| Shared diffuse textures | 146 texture/sampler pairs |
| Omitted references | Three initially disabled, one actor, 31 without supported generic MODL |
| Independent model comparison | All 218 samples match objects, materials, geometry and composed transforms |
| New BSXFlags projection | 175 blocks; exact name/index/value comparison, unknown bits preserved |
| Deliberate comparison defects | Nine cases rejected, including changed/missing extra flags |
| GPU captures | Two interior views and two refreshed model views; all exit codes zero |
| Source installation | Fresh full baseline matches all 464 files and original content fingerprint |

The deferred store indexes the same winning identities and group ancestry while
decoding TES4/CELL metadata. Selected bodies still use the strict, bounded decoder;
diagnostic checksum recovery is refused in this path. This permits inspection of a
valid cell without inflating unrelated terrain. It does not repair or certify the
known LAND payload. Full strict scans continue to fail that checksum.

`coordinates` keeps source reference math independent of Bevy. Static rotations use
the researched clockwise X/Y/Z convention. Source values remain unchanged; an f64
origin is subtracted before relative coordinates narrow to f32. Analytic tests cover
axis rotations, composition order, scale/translation and small offsets at large world
coordinates. This convention still needs measured NV retail comparisons. Selected
pinned OpenMW sources were consulted; no C++ implementation was copied or linked.

Presentation retains FormKey on each reference view. Models upload once and instances
reuse their handles. The optional fly camera supports source-coordinate starting views
and movement without collision. Bevy parent visibility is explicit, resolving the
hierarchy warning seen in the first exploratory capture. Declared BSX editor-marker
submeshes are omitted with diagnostics; standalone marker models remain visible.

The interior captures contain visible marker geometry and magenta material fallbacks
in the bedroom window. They establish a real cell assembly/GPU path, not completed
retail rendering. Actor/alternate-item models, enable-parent execution, controllers,
skinning, lighting/shaders, collision and measured movement remain open. Startup work
is synchronous; the debug smoke runs took about 42–43 seconds each, without a frame
rate or streaming benchmark. No gameplay scenario is accepted.

[interior-preview.json](interior-preview.json) binds the capture metadata, selected
plugin identities, exact selection comparison, model comparisons and negative checks.
Raw PNGs/models/textures remain in ignored `local/`. Reproduction commands and limits
are in [interior preview](../docs/interior-preview.md). Whole-corpus exceptions,
archive/loose precedence, the effective retail profile and all later brief milestones
remain listed in [NEXT_STEPS.md](../NEXT_STEPS.md).

Prior checkpoint evidence is preserved in [checkpoint 04](checkpoint-04.md),
[its verification](checkpoint-04-verification.json) and
[its source snapshot](checkpoint-04-source-snapshot.json). Current source/binary hashes
and the real implementation commit are recorded in [verification.json](verification.json)
and [source-snapshot.json](source-snapshot.json). The evidence commit follows the
implementation commit so its revision identity does not refer to itself.
