# Implementation checkpoint 02 ? cell dependencies and NIF containers

The Rust executable now follows a real New Vegas interior from winning plugin records
to placed references, base forms, and archived model candidates. It also inventories
all archived NIF/KF containers. This is working headless code; there is still no
rendered scene, movement, collision, script execution, or accepted gameplay scenario.
The original installation was not modified.

## Results

| Check | Observed result |
| --- | --- |
| Release build | Pass, Rust 1.99.0 / x86_64-pc-windows-msvc |
| `tools/check.ps1` | Pass: formatting, 29 tests (9 unit + 20 integration), Clippy with warnings denied |
| Doc Mitchell CELL | 435 winning placements; all base links resolve to allowed record kinds; one door link; zero link failures |
| Model dependencies | 222 distinct bases; 210 single archive candidates; 207 distinct model files; 12 bases without MODL |
| Interior NIF comparison | All 207 files / 3,819 ordered block entries and root lists equal to nifly |
| Cache reuse | All 207 model entries rehashed and reused; 7,613,455 decoded bytes |
| Older stream revision probes | One sample per eleven older revisions: 11 files / 763 block entries, all equal to nifly |
| Deliberate oracle mismatch | Changed one reported block size; comparison returned 1 and `all_equal=false` |
| Full archived NIF/KF census | 25,754 decoded of 25,760 members; 1,036,203 block occurrences, 159 distinct types |
| Remaining NIF format gap | Six version 20.0.0.4 members, individually reported as unsupported |

The complete inventory comprises 20,746 NIF and 5,014 KF members across 21 archives.
Twelve 20.2.0.7/user-11 Bethesda stream revisions were observed and retained explicitly.
All archive hashes in the new census can be tied back to the initial installation
baseline. Independent NIF comparisons cover 218 files / 4,582 block entries, not every
container in the census. Block internals remain unimplemented.

The cell command still returns 1 for the already documented base-plugin LAND checksum
mismatch. Its model and typed reference checks have no failures. The archive-level NIF
census returns 1 for six unsupported legacy containers. Those are distinct limitations;
none was hidden or reclassified as a passing gameplay test.

## Commands actually run

From `G:\Rust-Fallout`, with `<NV>` denoting the supplied installation:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\check.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools\cargo.ps1 build --release --locked -p fallout-cli
.\target\release\fallout.exe cell --install <NV> --load-order profiles\nv-inspection-order.json --editor-id GSDocMitchellHouse --inspect-checksum-mismatches --inspect-models --model-cache local\docmitchell-models --output local\cell-docmitchell-final.json
.\target\release\fallout.exe nif-census --install <NV> --output local\nif-census-expanded.json
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-nif-oracle.ps1
.\local\nif-oracle-build\Release\nif-oracle.exe local\docmitchell-models
py -3 tools\compare-nif.py --cell local\cell-docmitchell-final.json --oracle local\nif-oracle-docmitchell.json --cache local\docmitchell-models --output local\nif-comparison-docmitchell-final.json
```

The native oracle built with Visual Studio 18 / MSVC 19.50.35723.0 and SDK 10.0.26100.0.
It is a separate GPL executable; the Rust runtime has no nifly dependency. Upstream
fixture tests were not run or imported. Our comparison uses only authorized local
assets decoded into ignored cache directories. Nifly may prepare data in memory;
the driver never calls its save, optimization, or conversion functions.

## Tests and evidence limits

New synthetic cases cover indexed compressed reads and stale headers, closed group
ancestry, moved/deleted overrides, non-finite/truncated/duplicate placement fields,
wrong-kind and missing dependencies, unknown NIF block preservation, every truncated
fixture prefix, version/count budgets, root bounds, and observed stream identities.
The deterministic mutations are not a sustained fuzzing campaign.

Generated metadata is in [world-inspection.json](world-inspection.json) and
[NIF coverage](../parity/nif-coverage.json). Detailed behavior boundaries and exact
reproduction steps are in [world inspection](../docs/world-inspection.md).
The initial archive/plugin work remains recorded in [checkpoint 01](checkpoint-01.md).
[source-snapshot.json](source-snapshot.json) and [verification.json](verification.json)
bind current source files and the release binary; there is still no engine commit.

M1 remains open for effective mount precedence, typed field/asset dependency closure,
legacy NIF parsing and block payloads, SCDA decoding, retail oracle captures, and the
known compressed-source exceptions. M2 needs actual rendering, collision, and traversal.
No cell activation or campaign milestone has passed. Follow [NEXT_STEPS.md](../NEXT_STEPS.md).
