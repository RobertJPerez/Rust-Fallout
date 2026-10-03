# Preserving verified metadata

The initial census generator now requires a new staging directory directly under
the repository's `local/` directory. It cannot regenerate the initial profile,
command/condition registries or ledger over the current published files.

```powershell
py -3 tools/report.py --local local `
  --new-output-directory local/initial-census-replay
```

This reconstructs the initial census metadata from its retained local inputs.
It is historical evidence, not a current runtime verification or acceptance
report. Staged files have no verified implementation revision. Use the committed
checkpoint evidence workflow for new implementation claims.

The destination is checked before input processing and created exclusively before
publication. Existing destinations and individual output files are rejected.
Each generated file also uses exclusive creation. A failure can leave an owned
partial staging directory; it does not reset current metadata. Choose a new
directory for another attempt rather than treating partial output as verified.

Five asset-free Python regressions run through `tools/check.ps1`. They check the
required destination, occupied directories/files, repository/external targets and
the actual initial generator. The full successful fixture stages all seven JSON
outputs while preserving current profile/ledger/registries and input bytes.
Its census values are explicitly authored engineering fixtures.

Immutable checkpoint receipts and current aliases retain their existing separate
publication workflow. This fix does not grant gameplay acceptance or change any
original input file.
