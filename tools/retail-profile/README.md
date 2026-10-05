# Original profile capture

`fallout retail-profile-capture` captures a local New Vegas source profile and
prepares separate configuration/save directories for observation tooling.
It uses the existing `vfs::profile` parser, retaining original bytes, duplicate
INI entries, line offsets, empty lists and missing files.

```powershell
fallout.exe retail-profile-capture --install "G:\SteamLibrary\steamapps\common\Fallout New Vegas" --package "G:\Rust-Fallout-worktrees\integration\local\retail-profile-01"
```

The package parent must already exist. On Windows, omitted `--documents` and
`--local-appdata` resolve through .NET's Windows known-folder API. Explicit roots
are supported for authored fixtures and must be supplied on other platforms.
The tool never assumes `USERPROFILE\Documents`; OneDrive redirects matter.

The completed `capture.json` binds the producer binary, executable, full
installation fingerprint, ordered profile report and before/after fingerprints
of game user data, including existing saves. Configuration copies are checked
against those same manifests. The fresh staging area receives only configuration
files and an empty save directory. Existing saves and installation assets are
never copied, linked or modified. Captures contain private paths and raw settings;
keep the entire package in ignored local storage.

Installation walking retains the existing strict reparse-point policy. The
userdata walker separately permits resident Microsoft cloud placeholders with
verified cloud-family tags; symlinks, junctions, unknown tags and files needing
hydration refuse. Tag queries use the system `fsutil` program and fail closed if
its result cannot be classified. Userdata is limited to 65,536 entries and 32GiB.
Failed captures preserve partial evidence and do not publish a completed receipt.

This command does not launch the original executable. A copied configuration and
environment-variable changes do not redirect its Windows known-folder accesses.
`launch_admitted`, `runtime_ready` and gameplay acceptance stay false until a
separate-user or per-process isolation backend demonstrates actual opened paths
and writes. File observations alone do not determine effective settings, plugin
order, duplicate-key precedence or retail behavior. Trace importers can bind the
completed receipt's SHA-256 while retail execution remains unavailable.

`fallout retail-profile-verify` checks an existing package against an expected
receipt SHA-256 without reopening the original installation or user-data roots:

```powershell
fallout.exe retail-profile-verify --package "G:\Rust-Fallout-worktrees\integration\local\retail-profile-01" --receipt-sha256 <capture-json-sha256>
```

It checks report hashes, manifest digest recipes, the executable identity and
configuration copies. It reparses retained and staged configuration through the
production profile parser, including missing files, raw bytes, duplicates and
offsets. Package-relative evidence paths cannot escape the package or traverse
links, and staged saves must remain empty. Receipt reads are bounded to 1MiB and
each report to 32MiB. The result verifies retained profile evidence; it does not
remeasure current original inputs or authenticate an untrusted producer.

Use `--require-process` when consuming original execution evidence. Existing
profile-only captures always refuse that requirement, and altered receipts cannot
turn their process, isolation or gameplay flags into accepted evidence. An actual
isolated process capture needs its own demonstrated transport before these
requirements can pass.

Cloud-tag classification follows Microsoft's [reparse tag specification](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/c8e77b37-3909-4fe6-a4ea-2b9d423b1ee4).
