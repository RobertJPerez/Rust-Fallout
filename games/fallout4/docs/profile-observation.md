# Fallout 4 profile-file observation

`profile` reads an explicitly supplied profile directory's `plugins.txt` and
`loadorder.txt`, hashes their original bytes, and compares named plugin files
with regular-file candidates immediately under the supplied installation's
`Data` directory. It also hashes those direct plugin files and reports observed
modification times. Profile files are read through a byte limit and rejected if
their size or modification time changes during the read. Empty or missing
inputs, BOM/line endings, comments, literal leading `*` markers, duplicates,
invalid names, and files absent from the visible tree remain explicit in JSON.

The shared dependency is used for the FO4 profile identity and safe ASCII plugin
name normalization. The observation does not apply New Vegas resolution rules.
`*` is reported as a byte-level marker only. File order is retained as written;
the command does not assert the game's effective activation or finalized ESL
order. A missing Data-tree candidate may be supplied by a virtual mod deployment;
the observer does not search or infer one. It also does not resolve masters,
record winners, archives, VMAD attachments, or gameplay.

Build and inspect a selected profile with private one-job storage:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bin fallout4-prep
New-Item -ItemType Directory -Force local | Out-Null
.\local\target\debug\fallout4-prep.exe profile 'G:\SteamLibrary\steamapps\common\Fallout 4' "$env:LOCALAPPDATA\Fallout4"
```

The command writes JSON to stdout. Exit 2 means the observation is incomplete,
including missing profile files, malformed/duplicate rows, or named plugins
without one unambiguous regular-file candidate in `Data`. Exit 0 only means that
this bounded filename/profile observation completed; `runtime_ready` remains
false. Keep full reports under ignored `local/`, since they can contain a user's
mod names and source fingerprints.

On the 2026-10-04 inspection, the Vortex-generated `plugins.txt` contained 259
plugin rows (237 starred), and none matched a regular plugin file immediately
under the retail `Data` directory. `loadorder.txt` contained 438 rows: 16 matched
the 16 visible plugin files and 422 did not. A further 179 load-order names were
not listed in `plugins.txt`. Both profile files exactly match copies in the
Vortex profile directory, last modified 2026-04-27 03:27 UTC. `Fallout4.exe` and
`Fallout4.esm` in the inspected install were last modified 2026-10-03 05:14 UTC.
This profile snapshot predates the current install files by about five months,
so it cannot establish today's activation source. The private report records both
profile-file SHA-256 values, observed times, and each visible plugin-file hash at
`local/profile-observation-003/`.

This establishes a stale text snapshot and a mismatch with the visible
installation, not a broken game: a newer profile, virtual deployment, or a
different active profile may explain it. Until the effective profile source is
independently established, physical VMAD/PEX attachment findings cannot be
promoted to active-game failures.

A follow-up check on 2026-10-04 found two Vortex profile folders. The newer
folder's `plugins.txt` and `loadorder.txt` exactly match the global files; an
older folder has a separate 109-row plugin list. The only Vortex deployment
snapshot points to this same install root, but it was written on 2026-04-27. It
contains 10,181 historical paths and no path matching any of the 272 material
texture names without a direct BA2 candidate. The install currently has no
`Data/vortex.deployment.json` or the snapshot's historical
`Data/BCR/__folder_managed_by_vortex` marker, and no visible `Data/Textures`
directory. The first profile check used a conventional Documents path and did
not account for Windows' redirected Documents known folder. A follow-up resolved
that known folder and found `My Games/Fallout4` with three INIs; see the
[configuration observation](configuration-observation.md). The
`Fallout4Prefs.ini` timestamp is current to 2026-10-04, but it does not establish
which settings the game loaded. The additional local `DLCList.txt` is from
2026-05-11; `ContentCatalog.txt` and `UserDownloadedContent.txt` are from April.
These artifacts still do not tell us
which profile or virtual deployment is active for the October install. See the
[deployment checkpoint](../reports/deployment-checkpoint.json) for bounded
hashes and counts. Do not treat the historical snapshot as current activation.

A metadata-only recheck on October 5 hashed the global files, both known Vortex
profile copies and the deployment snapshot again. All seven file hashes still
match the prior report; the Vortex snapshot remains from April, no Vortex process
or install-side deployment marker was present during the check, and no profile
contents or mod names were written to the report. This narrows the known sources
without establishing which manager, profile or VFS the game used. Reproduce it
with a fresh private output directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/audit-profile-state.ps1 `
  -InstallRoot 'G:\SteamLibrary\steamapps\common\Fallout 4' `
  -OutputDirectory 'local/profile-recheck-NNN'
```

The exact observation and limitations are recorded in the
[current profile recheck](../reports/profile-recheck-checkpoint.json).
