# Authored NV collision decoding

The engine-independent Rust reader now preserves authored collision shapes, body
fields and shape links. Doc Mitchell's 207 model candidates contain 731 supported
collision blocks. Every projected field matched a separate raw nifly executable,
including stored float bits, block offsets, lengths and SHA-256 identities.
This is content-loading evidence. The inspection camera still passes through walls.

The reader lives in fallout-data's nif_collision module; it has no Bevy, Havok or
physics backend dependency. The sixteen implemented block names cover collision
attachments, rigid bodies, spheres, boxes, capsules, convex vertices and planes,
transform/list shapes, MOPP wrappers, packed shapes and packed triangle data. Source
coordinates and material/filter values remain unchanged. bhkRigidBodyT records that
its stored translation/rotation is active; decoding does not apply that transform.

The house contains 6,622 packed vertices, 2,810 packed triangles, 964 convex vertices
and 23,760 MOPP bytes. Degenerate triangles, winding, welding words, duplicate radius
and scale fields, unknown flag bits and alignment bytes remain intact. Matrix rows
retain their file order. No bounding-box replacement or visual-mesh collider is
generated. MOPP code remains opaque. Compressed vertices retain three source words
per vertex because their encoding is not certified as IEEE half precision.

Shape ownership is a directed acyclic graph. Different bodies and wrappers can
share a shape. An iterative walk visits disconnected components, rejects cycles,
and produces child-before-parent order in O(blocks + links) time. Attachments and
constraints have separate reference roles. Known wrong target kinds fail; known
unsupported kinds and unclassified kinds remain explicit links, with type_verified
distinguishing them. Unsupported blocks never become physics-ready.

Each supported payload must consume its complete block. Reference indices,
triangle indices, canonical booleans and meaningful finite floats are checked.
Alignment words remain raw bits, including values that would be NaN if interpreted
as floats. The decoder accepts only the container reader's twelve observed NV
20.2.0.7/user-11 stream revisions. Six legacy containers still need another reader.
Input, block-count and owned array-element limits default to 64 MiB, 100,000 blocks
and 128 MiB. The CLI also caps retained collision collections at 256 MiB per batch.
That estimate accounts for collection capacity and owned payloads; temporary
container/graph tables, JSON storage and allocator bookkeeping have separate limits
or are outside the estimate. It is not a peak-process-memory guarantee.

## Inspect local model bytes

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli
.\target\release\fallout.exe nif-collision .\local\docmitchell-models --output .\local\collision-new.json
~~~

The input may be one NIF/blob or a directory of regular .nif/.blob files.
The command reads source handles without writing to them, reports each failure and
returns 1 if any decode or comparison fails. Output files must be new and outside
the input directory. Raw reports contain extracted geometry and stay under ignored
local/; the committed [collision report](../reports/nif-collisions.json) contains
only counts, source identities and comparison metadata.

## Independent evidence

The separate GPL tool loads pinned nifly factories directly. It bypasses
NifFile::Load and PrepareData, snapshots each input, and hashes the exact source
and block spans through Windows CNG. All source floats are emitted as IEEE binary32
bits. Rust compares those bits and every projected integer, flag, index, reference
and source boundary exactly. There is no numerical tolerance for this comparison.
This tests our parsing against another reader; it does not establish retail physics.

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-nif-oracle.ps1
~~~

The native oracle accepts a file or directory followed by --collision. Its JSON
must be saved without PowerShell's legacy text-redirection encoding. The Rust
evidence runner captures it as bytes, invokes the release CLI with --oracle-report,
runs the workspace checks and rehashes the original installation:

~~~powershell
# Commit the implementation first; choose a fresh local run directory.
.\target\release\fallout-evidence.exe --run-directory .\local\collision-07-verified --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
~~~

This checkpoint runner publishes immutable checkpoint-07 receipts and
reports/nif-collisions.json, refusing existing output files. It binds each
source/configuration file to the implementation commit, verifies executables stay
unchanged during the run, and compares every installation-file hash with the first
baseline. It is deliberately specific to this checkpoint, rather than a generic
acceptance framework.

Twelve authored integration tests cover source fields, compressed-word retention,
shared shapes, cycles, a 10,000-wrapper chain, truncation/surplus payloads, wrong
references, nonfinite data, budgets and 512 deterministic byte mutations. The CLI's
independent comparison test also rejects nine deliberate metadata/data changes,
including signed zero and triangle winding. Authored fixtures contain no retail
bytes. Eleven older-stream sample files also compare successfully, containing three
supported collision blocks: one box, one transform and one SPCollisionObject. Eight
of those samples have no implemented collision blocks; their comparison only checks
the source/container identity and empty projection. Fifteen of sixteen implemented
names have a compared retail sample. bhkPCollisionObject has authored tests only.
Per-type sampled counts are recorded in the report.

Layout facts were read from pinned
[nifxml](https://github.com/niftools/nifxml/tree/970a6238218a106daaeb89a61bcda0eeaf9d08c4).
The oracle uses pinned
[nifly](https://github.com/ousnius/nifly/tree/cca0a770094bb962fb28ea1fec5ea903e68fda8e).
The original Rust decoder does not include their implementations or generated XML
code. The offline C++ projection and nifly remain separate from the runtime.
The oracle's SHA-256 operation follows Microsoft's
[BCryptHash documentation](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcrypthash).

## Remaining work

The house still contains nineteen unsupported constraint blocks and eighteen blend
controllers. No constraint solver, body activation, collision query, character
controller or Havok behavior adapter exists. Axes, rendering-to-Havok units,
collision margins and placement composition require measured retail fixtures.
The compressed packed-vertex branch has authored tests only; pinned nifly does not
read that layout correctly and cannot independently certify it. Whole-corpus
collision coverage and other-game variants remain open. M1 and M2 are unfinished.
