# Real cell and NIF inspection

The `cell` command is the next working layer above the initial content census. It
loads the explicit plugin order, finds a CELL by editor ID, follows winning child
records, and reads their bodies on demand. The first measured cell is
`GSDocMitchellHouse`, FormKey `nv-original:falloutnv.esm:103DF9`.

## What the cell path does

Group ancestry preserves world labels, cell labels, and child kinds 8/9/10. Group
closure prevents one cell's membership leaking into the next record. Parent IDs use
the defining plugin's master table. A moved winning override belongs to its new cell;
a deleted winner remains a tombstone rather than resurrecting its predecessor.

The store keeps Windows read handles open from index construction through its last
record access. Reads seek directly to indexed headers, verify those headers still
match, and reuse the same bounded decompressor as sequential scans. Unknown fields
remain available in the original record body. This index is in memory; a reusable
serialized index cache is not implemented yet.

Decoded fields are CELL DATA/XCLC/FULL and REFR/ACHR/ACRE NAME/DATA/XSCL/XESP/XTEL.
Transform components must be finite; singleton fields reject duplicates. Links check
the expected target record kinds from the pinned xEdit FNV schema. A present SCPT is
not accepted as an object's STAT/NPC base or a REFR door target. Each decoded field
retains its offset within the decompressed record body. Position/rotation values
remain in original units; no unverified engine-axis conversion has been applied.

The report retains initially-disabled and persistent flags. It does not evaluate
enable parents, execute scripts, teleport actors, or resolve every ownership/leveled/
template dependency. Door targets are checked as REFR records; door runtime rules
and destination-cell behavior are still future work.

## What was observed

Doc Mitchell's house has 435 placed references with 435 resolved, correctly typed
base links and one resolved door link. Its 222 distinct base records expose 210 MODL
paths with a single archive candidate, corresponding to 207 distinct model files.
Twelve bases have no MODL; the report does not invent a model for them. One NAVM child
is counted without decoding its navigation semantics. See
[the generated summary](../reports/world-inspection.json).

All 207 model files decode and pass the new NIF container checks. Their 3,819 block
type/index/size entries and footer roots match independent nifly output exactly.
The probe reads 7,613,455 decoded model bytes. A second run verified and reused all
207 cache entries. The base plugin's known LAND checksum defect is still reported,
so the overall diagnostic command returns 1 and `runtime_ready` remains false.

These are archive candidates, not an inferred retail override policy. Models with
multiple candidates would remain ambiguous. No archive/loose winner is manufactured.
Effective mount precedence and alternate actor/item model selection remain open.

## NIF/KF scope

The reader handles the outer 20.2.0.7 container, little endian, user version 11, and
the observed Bethesda stream revisions 14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33,
and 34. It validates table allocation budgets, type indices, block extents, string
tables, footer roots, and exact file consumption. Unknown blocks retain exact byte
ranges; their payloads are not silently discarded or interpreted as geometry.

The complete archive census found 20,746 NIF and 5,014 KF members. 25,754 containers
decode, containing 1,036,203 block occurrences of 159 distinct types. Six members use
the older 20.0.0.4 format, which lacks the same block-size table and needs its own
parser path. They are listed individually in [NIF coverage](../parity/nif-coverage.json).
Counts are archive-member occurrences, including duplicate paths across archives.

One real sample from each of the eleven older stream revisions was independently
compared with nifly: 11 files and 763 block entries, all equal. Together with the
interior models this is 218 independently compared files, not a claim that every
container in the corpus received an independent NIF comparison. A deliberately
altered block-size result produced a failed comparison and exit code 1.

The separate [scene decoder](nif-scenes.md) now reads supported node links,
transforms, and triangle vertex/index buffers, with raw independent comparisons.
The [material extension](nif-materials.md) now follows external texture paths, and
the [model preview](model-preview.md) renders individual supported models. Skinning,
controllers, collision shapes and complete cell assembly remain unimplemented.
A block type in the census does not imply support for that block's runtime behavior.

## Reproduce the independent comparison

After producing a new cell report and its model cache as shown in the README:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\build-nif-oracle.ps1
.\local\nif-oracle-build\Release\nif-oracle.exe .\local\docmitchell-models | Set-Content -Encoding UTF8 .\local\nif-oracle-new.json
py -3 .\tools\compare-nif.py --cell .\local\cell-new.json --oracle .\local\nif-oracle-new.json --cache .\local\docmitchell-models --output .\local\nif-comparison-new.json
```

The comparator rehashes cached bytes before comparing exact version tuples, ordered
block tables, and roots. Use a cache directory dedicated to the report; unexpected
oracle files fail the comparison. Existing comparison outputs are never overwritten.
The oracle is a separate GPL C++ executable, not a dependency of the Rust runtime.
Its upstream loader performs in-memory preparation; no save, optimization, or file
conversion is invoked by this driver. The comparison covers container metadata only.
