# Fallout 4 configuration-file observation

The configuration audit resolves Windows' `Documents` known folder, then reads
only `Fallout4.ini`, `Fallout4Prefs.ini` and `Fallout4Custom.ini` from its
`My Games/Fallout4` child. On this machine the known folder is redirected to
OneDrive, so checking only the conventional local `Documents` path missed the
profile directory. The follow-up found all three INI files; `Fallout4Prefs.ini`
was modified at 2026-10-04 03:22 UTC, while the other two were last modified on
2026-04-27.

The audit preserved byte-exact copies under ignored `local/user-config-001/`.
Its private manifest records source and copy hashes, sizes, paths and times.
Git-tracked evidence reports only file names, sizes and timestamps; setting
values and individual user-file hashes remain private. All three source and copy
hashes match, and no source file was modified.

Run it from the repository root with a fresh ignored output directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/audit-game-config.ps1 `
  -OutputDirectory 'local/user-config-NNN'
```

The tool uses Windows' current Documents known folder by default. An explicit
`-ProfileDirectory` can be supplied when auditing a different known profile. It
accepts only the three named INI files, rejects reparse-point inputs and files
over one MiB, and writes no settings text to stdout or the public checkpoint.

These files are a configuration candidate, not proof that Fallout 4 loaded
them. The audit does not interpret INI keys, inspect command-line overrides,
identify the active plugin profile, resolve Vortex or another virtual filesystem,
or establish graphics/gameplay behavior. See the
[profile observation](profile-observation.md) and
[configuration checkpoint](../reports/configuration-checkpoint.json).
