# Rust Fallout

Robert Perez's standalone Rust Fallout runtime project, starting with New Vegas.
The full direction is preserved in [the master brief](docs/references/Fallout_Rust_Codex_Master_Brief.txt).
Original-game profiles come first; FO3/TTW, FO4, FO76 research, and a separate crossover
profile remain in scope. Their runtime support is not implemented yet.

**The content pipeline now renders a real interior.** It reads the installation
directly, decodes scene geometry and material fields, resolves archived textures,
and assembles Doc Mitchell's house from winning plugin references in Bevy. The CLI also provides corpus inspection,
structural override resolution, script-reference inventory and a verified asset cache.
The cell inspector can also reuse source-bound plugin indexes, checking source and
cache hashes while retaining strict on-demand record reads.
The terrain inspector now follows exterior cells, parent worlds and winning LAND
fields. See [exterior source inspection](docs/exterior-fields.md) for Goodsprings
and DLC comparisons, source hashes and the remaining terrain work.
An optional height stage reconstructs LAND grids and diagnoses neighboring cell
edges while preserving source fields. See [terrain heights](docs/terrain-heights.md).
The source mesh stage and --terrain preview now display real Goodsprings and DLC
surfaces with authored normals/colors. See [terrain geometry](docs/terrain-geometry.md)
for reproduction commands and the unlit inspection scope.
An optional texture stage now follows authored nonnull LAND layers through winning
LTEX/TXST records to verified archive members. It retains unapplied NULL defaults;
see [terrain texture dependencies](docs/terrain-textures.md).
Sparse terrain alpha samples also expand into independently compared quadrant
weights; see [terrain blend maps](docs/terrain-blends.md).
Goodsprings and two other source cells now render authored diffuse terrain layers.
See [the textured build and manual test](docs/terrain-textured-preview.md).
The inspection view draws 400 of 435 references using shared models and textures.
It now handles authored untextured materials and source alpha, culling and depth
states; 64 synthetic GPU checks now cover render states and terrain weight passes.
Authored collision shapes and rigid-body fields now decode independently in Rust;
731 collision blocks in the house match raw nifly output exactly.
The compiled-script census now frames all 14,514 authored bodies without needing
source text. An independent offline reader matches 142,218 instruction headers;
operands and execution remain open. See [compiled scripts](docs/compiled-scripts.md).
The offline command catalogue reads the fingerprinted executable and independently
checks 640 command, 38 event and 16 statement descriptors.
See [command metadata](docs/command-catalogue.md) for argument signatures and scope.
The script table decoder preserves 80,736 embedded/standalone units and independently
checks all 28,463 top-level caller associations. Original stale counts and repeated
variable indices remain explicit; see [script bindings](docs/script-bindings.md).
Expression inspection now independently checks 53,404 envelopes and 149,082 tokens,
including 16,320 embedded calls. Arguments and evaluation remain unfinished; see
[compiled expressions](docs/script-expressions.md).
Retail rendering, physics, player simulation, combat, dialogue, the script VM and
saves remain unfinished. No campaign or gameplay scenario has passed acceptance.

The source is published to [RobertJPerez/Rust-Fallout](https://github.com/RobertJPerez/Rust-Fallout).

## Run it

From `G:\Rust-Fallout`, using PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\cargo.ps1 build --release --locked -p fallout-cli
.\target\release\fallout.exe --help
.\target\release\fallout.exe inspect 'G:\SteamLibrary\steamapps\common\Fallout New Vegas\Data\CaravanPack.esm' --form 001735DD
```

Open the Vit-o-matic model from your installation:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\cargo.ps1 build --locked -p fallout-preview
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --model meshes/architecture/goodsprings/nv_vitomaticvigortester_cabinet02.nif
```

A/D orbit, W/S tilt, Q/E zoom, R resets and Escape closes. See
[model preview](docs/model-preview.md) for GPU captures and the current rendering
limits. It uses unlit diffuse materials; retail lighting and shader parity are open.

Open the real interior with a source-coordinate camera:

```powershell
.\target\debug\fallout-preview.exe --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --cell GSDocMitchellHouse --load-order profiles/nv-inspection-order.json --camera-position 2130 2130 7440 --camera-look-at 1883 1763 7420
```

Tab switches between orbit and fly. In fly mode, WASD moves, Q/E moves vertically,
arrow keys look, and Shift increases speed. This camera passes through geometry;
it is an inspection tool. [Interior preview](docs/interior-preview.md) records the
captures, omissions, provisional placement convention and remaining material gaps.

For a corpus-wide inspection, use these commands; change the output name if it already exists:

```powershell
$install = 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
.\target\release\fallout.exe baseline --install $install --output .\local\baseline-new.json
.\target\release\fallout.exe census --install $install --inspect-checksum-mismatches --output .\local\census-new.json
.\target\release\fallout.exe resolve --install $install --load-order .\profiles\nv-inspection-order.json --inspect-checksum-mismatches --output .\local\resolution-new.json
.\target\release\fallout.exe plan --install $install --load-order .\profiles\nv-inspection-order.json --inspect-checksum-mismatches --output .\local\plan-new.json
```

The diagnostic commands write their reports and **exit with code 1** on this corpus:
the base ESM contains a checksum mismatch in LAND `00150FC0`. Without the diagnostic
flag, parsing stops at that record. This is an observed vanilla defect, documented
in [format exceptions](docs/format-exceptions.md); it is not treated as accepted data.
The supplied inspection order is explicit test input, not a claim about the retail
game's effective load order. These commands never change that order.

Decode and cache a real asset:

```powershell
New-Item -ItemType Directory -Force .\local\cache | Out-Null
.\target\release\fallout.exe asset "$install\Data\CaravanPack - Main.bsa" 'meshes/nvdlcpre3/weapons/2handrifle/caravanshotgunpreorder.nif' --cache-root .\local\cache
```

This verifies bytes; it does not render or interpret the NIF. Cache entries include
the source archive digest, profile, member path, and decoder version. Reuse verifies
the manifest and blob digest. Outputs must be outside the installation, and report
files are created without overwriting existing files.

## Build and check

The workspace pins Rust **1.99.0**, edition 2024, on `x86_64-pc-windows-msvc`.
Visual Studio C++ tools and the Windows SDK must be installed. They are present on
this machine. Rust lives under `.tools/`; the scripts do not change the system PATH.

```powershell
# On a fresh checkout, install the local Rust toolchain first:
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\bootstrap.ps1
New-Item -ItemType Directory -Force .\local | Out-Null

# Formatting, synthetic tests, and Clippy:
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\check.ps1

# Independent retail archive and plugin comparisons; reads the whole corpus:
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\compare-corpus.ps1
```

The archive comparison exits unsuccessfully for two malformed text entries in
`Fallout - Misc.bsa`, while preserving its results. It does not hide those failures
to make the run green. The GPL esplugin oracle is a separate workspace and executable;
it is not linked into `fallout`. The ba2 archive oracle is tooling only.

## Verified here

The [corpus report](reports/corpus.json) and [checkpoint report](reports/checkpoint.md)
record the evidence and its limits:

- All 464 installation files hashed: 9,907,238,722 source bytes.
- All 10 official plugins framed: 629,788 records excluding TES4 headers, with
  one explicitly tainted diagnostic record. Counts and masters agree with esplugin.
- All 21 BSA indexes compared: 182,177 member paths/hashes; 182,175 payloads byte-equal
  between independent readers, with two decode disagreements.
- 628,464 definition identities and 780 override chains; 44,508 SCRO record links
  and 4,566 references to the engine-provided player. No remaining missing SCRO target.
- 148 condition IDs across 80,627 CTDA occurrences. Conditions are not evaluated.
- Doc Mitchell's real cell: 435 placements with valid base-form links, one resolved
  door destination, and 207 distinct archived models inspected. Those models' 3,819
  block entries and roots agree exactly with an independent nifly executable.
- Scene nodes and triangle data now decode independently in Rust. The house's 744
  objects, 401 meshes, 96,437 vertices and 84,403 triangles match raw nifly output;
  source attributes compare by their exact f32 bits. Composed transforms also agree.
- 25,754 of 25,760 archived NIF/KF containers decode across twelve observed stream
  revisions. Six legacy containers remain unsupported. The supported scene payloads
  decode in 25,729 files; 25 files fail numeric/index checks. This does not establish
  shader, animation, collision, or rendering support.
- 1,127 material blocks match the raw oracle across 218 models. The house's 854
  external texture references resolve to 312 unique textures with verified cached bytes.
- Bevy GPU captures show the real chair, Vit-o-matic and two views inside the house.
  Its 400 rendered references share 203 models and 146 texture/sampler pairs.
  Authored untextured window/shadow bindings now render without magenta fallback;
  standalone heading/audio markers remain visible.
- Strict deferred indexing reproduces all selected cell fields while leaving 585,196
  other record bodies explicitly unvalidated. Access still enforces normal strict checks.
- 158 tests pass, including malformed compression, forward/cyclic links,
  master ordering, identity collisions, deterministic plans, and interrupted cache publication.
- 64 asset-free GPU cases pass for source alpha comparisons, blend factors, face
  culling and depth states. [Material states](docs/material-states.md) records their scope.
- Authored collision payloads match the raw oracle across the house's 207 models:
  6,622 packed vertices, 2,810 packed triangles and 964 convex vertices. Source units
  are retained; physics and movement remain open. [Collision decoding](docs/nif-collisions.md)
  records the exact scope and reproduction commands.
- All ten plugin metadata indexes persist and reuse across launches, covering
  629,788 definitions in about 30 MB. Cached and uncached cell fields agree exactly;
  killed publication workers recover without changing sources.
  [Record index caching](docs/record-index-cache.md) records the scope and commands.
- Six source terrain meshes match original C++ projection exactly: 6,534 positions
  and normalized normals, 12,288 triangles and 5,445 authored color triplets.
  Five terrain GPU captures and one interior regression complete the inspection smoke checks.

## Inspect a real interior and its models

```powershell
$install = 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
New-Item -ItemType Directory -Force .\local\docmitchell-models | Out-Null
.\target\release\fallout.exe cell --install $install --load-order .\profiles\nv-inspection-order.json --editor-id GSDocMitchellHouse --inspect-checksum-mismatches --inspect-models --model-cache .\local\docmitchell-models --output .\local\cell-new.json
.\target\release\fallout.exe cell --install $install --load-order .\profiles\nv-inspection-order.json --editor-id GSDocMitchellHouse --defer-unread-payloads --output .\local\cell-strict-new.json
.\target\release\fallout.exe nif-census --install $install --output .\local\nif-census-new.json
```

The cell command reads winning records on demand and reports original transforms,
source offsets, flags, typed links, and model candidates. It returns 1 for the existing
LAND integrity issue when fully indexing payloads. The deferred command returns 0
for this cell's strict selected reads; it does not certify unread LAND data or repair
the installation. The NIF census returns 1 for six unsupported legacy files.
Neither command activates a cell or modifies the original game. Exact comparison
commands and limits are in [world inspection](docs/world-inspection.md).

Decode a cached model's scene and geometry, or inventory supported payloads across
all archives:

```powershell
.\target\release\fallout.exe nif-scene .\local\cache\7c8d4929ebfc958d9feb18adcd5ec56d7f81681e14799d144d04106fe0882ba0.blob --output .\local\shotgun-scene.json
.\target\release\fallout.exe nif-census --install $install --inspect-scenes --output .\local\scene-census-new.json
```

The scene output contains source arrays and composed transforms; large meshes produce
large JSON reports. Unknown blocks remain indexed and reported. The full scan returns
1 for its recorded failures. [Scene decoding](docs/nif-scenes.md) explains the limits
and independent comparison commands.

## Code layout

`fallout-data` owns format framing, immutable source identity, bounded archive access,
structural content resolution, preparation plans, and cache publication. `fallout-cli`
exposes those operations. The independent comparison drivers live under `tools/`.
`fallout-preview` owns the Bevy adapter, image upload and inspection camera.
Modules will become separate crates when real consumers justify that boundary.

Canonical gameplay state will live outside the presentation ECS. The current
[runtime decision](docs/decisions/0001-runtime.md) uses Bevy services for presentation;
game-specific decoding and future canonical simulation remain independent of its ECS.

Start with [NEXT_STEPS.md](NEXT_STEPS.md) for the next unmet gate. The
[parity ledger](parity/requirements.json), [profiles](docs/profiles.md),
[source audit](docs/source-audit.md), and [format contracts](docs/contracts.md)
distinguish tested behavior, decoded metadata, and unfinished work. No completion
percentage is inferred from file counts or tests.
