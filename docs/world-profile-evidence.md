# Candidate NV configuration evidence

`fallout vfs-profile` reads explicit installation, Documents and local-appdata
roots. Supply the Windows Known Folder Documents root, which can be redirected;
the inspector never substitutes `USERPROFILE/Documents`. Empty root paths are
rejected rather than resolved against the working directory.

```powershell
fallout vfs-profile --install $install `
  --documents ([Environment]::GetFolderPath('MyDocuments')) `
  --local-appdata $env:LOCALAPPDATA --output local/profile-evidence.json
```

The seven fixed candidates are installation defaults, Documents `Fallout.ini`
and `FalloutPrefs.ini`, local-appdata `plugins.txt`, `NVDLCList.txt` and
`loadorder.txt`, and `Data/ArchiveInvalidation.txt`. The last is a candidate
location, not an implemented interpretation of `SInvalidationFile`. No path read
from an INI is followed. Reports must be outside all three input roots and never
overwrite an existing file. Raw configuration reports belong in private evidence;
public source names are logical root/relative names without private absolute paths.

`vfs::profile::observe` records each candidate's missing/empty/present state,
exact size and SHA-256, original bytes, and physical lines with byte spans and
original terminators. UTF-8 BOMs are preserved. Eight-bit bytes remain bytes;
their code page is not guessed. UTF-16/32 and unsupported control bytes are
explicit errors. Settings retain section, key and value spans without unquoting
or removing inline text. Unknown syntax remains unparsed source evidence.
Duplicate diagnostics use ASCII-folded section/key names and refer to the first
physical occurrence in that file; every occurrence stays in order. This
diagnostic convention is not a retail merge or last-wins rule. List entries keep
their markers, spelling, repetition and order without interpreting activation.

Limits admit seven files, at most 1 MiB per file and 2 MiB total, 16,384 physical
lines, 8,192 settings/list entries and 2 MiB of attempted normalized identifier
copies. Callers may lower these ceilings. Admission happens before retaining the
corresponding bytes/entries. These are source/work bounds; collection and file
handle overhead are additional. Windows source handles deny writes until the
snapshot drops. Other platforms retain the existing immutable-source contract.
Missing inputs are observations during inspection, not an atomic profile census.

The named consumer emits candidate source evidence with runtime consumption,
cross-file precedence, active plugin/DLC order, auto-mounting, archive/loose
precedence, invalidation and binding interpretation unresolved. It does not change
configuration, installation data, canonical state, cache/store/index policy or
strict compression checks. A successful evidence report is not an activated
profile, a content-acceptance result or a gameplay parity claim.

WORLD-02 also reproduces the three pinned compression exceptions independently
in private evidence: two incomplete Misc streams and the LAND checksum mismatch.
Their diagnostic bytes match earlier independent results. Retail handling still
requires evidence; these observations do not admit malformed input to production.
