# Recursive Fallout 4 Data inventory

The original retail census selected top-level files only. The follow-up audit
recursively inventoried the physical `Data` directory at
`G:\SteamLibrary\steamapps\common\Fallout 4`. It recorded 121 files totaling
38,242,910,044 bytes: 107 direct files and 14 nested `.bk2` videos under
`Data/Video` (4,229,503,352 bytes). All 107 direct-file hashes match the frozen
`local/proof-fo4-002/census.json` census. The 14 nested videos were hashed before
and after the full pass and rehashed during finalization. The retail source files
were not modified.

The detailed file list and completion evidence live under ignored
`local/recursive-data-002/`; the public summary and artifact hashes are in
[`reports/data-tree-checkpoint.json`](../reports/data-tree-checkpoint.json).
The inventory's SHA-256 is
`0d19ef7351df13207b189e57985af4148553fd0926398b905531019a969ec782`.

To repeat this scan, run from the repository root with a fresh evidence folder.
The initial pass fingerprints every file twice and saves the ledger; finalization
rechecks that the current Data path set and file sizes still match, compares all
direct files with the frozen census, rehashes each nested file, and writes the
completion marker last:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/audit-recursive-data.ps1 `
  -InstallationRoot 'G:\SteamLibrary\steamapps\common\Fallout 4' `
  -CensusPath 'local\proof-fo4-002\census.json' `
  -OutputDirectory 'local\recursive-data-NNN'
py -3.13 tools/finalize_recursive_data.py `
  'G:\SteamLibrary\steamapps\common\Fallout 4' `
  'local/proof-fo4-002/census.json' `
  'local/recursive-data-NNN'
```

Do not reuse an output directory. A failed scan leaves an incomplete folder;
preserve it and choose a new name for a rerun. This is a physical directory
inventory only. It does not observe Vortex or another virtual filesystem,
establish an active profile or plugin order, identify effective archive winners,
or show which assets the game uses. It is not a gameplay, native API or save
compatibility test.
