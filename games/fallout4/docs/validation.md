# Reproducing the checks

Run from `G:\Rust-Fallout4`. The main workspace has no retail-data dependency
for its tests. The offline archive comparator is a separate Cargo workspace so
its C++ DirectXTex/zlib/LZ4 dependencies cannot enter the production binary.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 fmt --all -- --check
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 test --locked --jobs 1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 clippy --locked --all-targets --jobs 1 -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --bins --jobs 1
```

For private retail comparisons, obtain the pinned external reader in an ignored
directory (the build script rejects the wrong revision or a dirty checkout):

```powershell
git clone https://github.com/Orvid/Champollion.git local/research/Champollion
git -C local/research/Champollion checkout bc961a0bdfb4831f8240e6dacee0818b4bf81e00
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-pex-oracle.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --manifest-path tools/ba2-oracle/Cargo.toml --locked --jobs 1
.\local\target\debug\verify-retail.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local/proof-new .\local\pex-oracle-build\Release\fo4-pex-oracle.exe .\local\target\debug\fo4-ba2-oracle.exe
```

The output directory must be new. The runner executes sequentially, fingerprints
its executables before/after, hashes every selected source file, compares BA2
members, extracts archived PEX into private numbered directories, runs the external
PEX reader, and checks its results against the Rust census. `complete.json` is
written last. A failed/interrupted run retains its diagnostics without a completion
marker. A crash during the marker write may leave invalid JSON; only a fully parsed
`complete.json` with matching referenced evidence is a completed proof. No power-loss
durability guarantee is claimed. Do not rebuild binaries or update game files during a proof.

BA2 scope: all named entries are matched by stored hash and exact original filename
bytes in both readers. Recomputed path-hash disagreements are separately retained
as unresolved findings, and their payloads are always compared. This avoids losing
physical entries when an encoding-dependent name does not reproduce the stored hash.
Every base Misc member is byte-compared; other archives compare the first
four members, every 997th member, and every path-hash discrepancy. Texture comparison includes reconstructed DDS
headers and payload, with one named normalization: unused non-volume `dwDepth`
values 0/1. The two writers differ there; both the volume/depth flags and DX10
resource dimension are checked before allowing this difference. Every other header
and payload byte must match. The report counts these normalizations separately.
Microsoft documents `dwDepth` as unused outside volume textures in
[DDS_HEADER](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-header)
(page updated 2021-03-15, inspected 2026-10-03). Tests ensure this cannot hide a
volume, dimension, format or payload mismatch. These are samples, not full asset
byte/pixel parity. No production bytes are changed by the comparison policy.

Initial PEX proof scope: every physical archived script is walked by both readers. The comparator
checks string/object/struct/variable/property/state/function/instruction counts,
opcode histograms, and every native class/parent/state/name/flags/return type and
parameter signature. SHA-256 ties the extracted files to the original census.
That initial proof did not compare all operand values, debug contents or VM behavior. Negative
tests deliberately remove/change expected comparison fields and must fail.

The incremental definition/operand proof reuses those immutable extracts, checks
their hashes against the frozen census and compares 4,067,308 semantic tokens
across all 10,342 PEX files. It includes the complete string table, user flag
table, classes, structs/members, variable defaults, properties, backing names,
accessor bodies, states, functions, parameters/locals and every instruction
operand/vararg. Floating values are compared by bits. Headers/debug metadata,
operand type rules, executable linking and runtime behavior remain outside that
comparison. See docs/papyrus-linking.md for the command and static binding scope.

Plugin scope: the existing bounded reader frames all physical records/subrecords,
validates zlib, and records masters, versions and VMAD headers. FO4 field meanings,
effective overrides and ESL identities still need an independent semantic oracle
beyond the separately verified VMAD fields (docs/vmad.md).
Header count discrepancies are retained as findings, not used to truncate the walk.

Private evidence contains extracted proprietary content and stays under `local/`.
Only aggregate results, source fingerprints and known gaps belong in the public
checkpoint. The selected baseline covers `Fallout4.exe` and top-level Data files;
loose-file subdirectories, user settings, active plugins and saves are outside it.
