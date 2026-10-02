# Rust Fallout

Robert Perez's standalone Rust Fallout runtime project, starting with New Vegas.
The full direction is preserved in [the master brief](docs/references/Fallout_Rust_Codex_Master_Brief.txt).
Original-game profiles come first; FO3/TTW, FO4, FO76 research, and a separate crossover
profile remain in scope. Their runtime support is not implemented yet.

**The content pipeline now has a real model preview.** It reads the installation
directly, decodes scene geometry and material fields, resolves archived textures,
and renders individual models with Bevy. The CLI also provides corpus inspection,
structural override resolution, script-reference inventory and a verified asset cache.
Full interiors, player movement, combat, dialogue, the script VM and saves remain
unimplemented. No campaign or gameplay scenario has passed acceptance.

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
- Bevy GPU captures show the real chair and Vit-o-matic using archived diffuse textures.
  This is a model inspection view; complete interior and retail visual parity remain open.
- 47 synthetic tests pass, including malformed compression, forward/cyclic links,
  master ordering, identity collisions, deterministic plans, and interrupted cache publication.

## Inspect a real interior and its models

```powershell
$install = 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
New-Item -ItemType Directory -Force .\local\docmitchell-models | Out-Null
.\target\release\fallout.exe cell --install $install --load-order .\profiles\nv-inspection-order.json --editor-id GSDocMitchellHouse --inspect-checksum-mismatches --inspect-models --model-cache .\local\docmitchell-models --output .\local\cell-new.json
.\target\release\fallout.exe nif-census --install $install --output .\local\nif-census-new.json
```

The cell command reads winning records on demand and reports original transforms,
source offsets, flags, typed links, and model candidates. It returns 1 for the existing
LAND integrity issue; the NIF census returns 1 for six unsupported legacy files.
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
