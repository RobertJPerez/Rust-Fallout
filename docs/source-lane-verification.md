# Verifying the integrated source lanes

The actor and asset workers use separate branches, output targets and raw evidence
directories. Their tested commits are reviewed and integrated by the primary
agent. A worker comparison remains development evidence until a fresh run binds
the integrated source and the binaries actually used.

`tools/verify-source-lanes.py` prepares that shared proof. It requires a clean,
committed working tree and new evidence directories immediately under `local`.
Its source snapshot checks every selected source/tooling file against the commit.
The release CLI, runtime evidence runner and both dedicated native oracles are
hashed before execution and checked again at the end. Workers can keep editing
their external worktrees; the primary tree and its proof binaries remain frozen.

The actor comparison reads the original plugins independently in C++ and compares
every retained physical field, scalar occurrence and source identity. Cold, warm
and reordered runs must keep the same full source cohort and actor definitions.
A changed fatigue word must fail comparison. Version, truncation, signed-bit,
duplicate, missing-field and budget boundaries also have focused Rust coverage.

The skin comparison first runs 48 authored files across all 12 admitted stream
revisions and raw weight-presence bytes 0, 1, 2 and 255. It checks 24 altered
reports, including fields that the pinned nifly reader normalizes. A fresh bounded
archive selection then feeds original source bytes through the native oracle and
production Rust decoder. Exact file hashes, spans, arrays and geometry owner
associations must agree. Selection findings and unresolved dependencies remain
in the receipt.

An oracle's syntactically valid digest does not establish which executable ran.
The integration runner compares that embedded digest with the actual binary hash.
It separately rejects a changed, valid-length digest. This is a runner identity
check; the standalone skin inspector validates the report contract and fields.

Finally, the runner executes the existing checkpoint 43 runtime verification
surface with `--no-publish` against the same integrated source. That selector
identifies the regression scope. Its local receipt is not a replacement for the
published checkpoint 43. The new integration receipt records the regression
revision, source digest, report hash and full installation baseline check.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --release --locked -p fallout-cli
powershell -NoProfile -ExecutionPolicy Bypass -File tools/actor-oracle/build.ps1 -BuildDirectory G:\Rust-Fallout\local\actor-oracle-build
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-nif-skin-oracle.ps1
# Commit source before starting; use fresh names for both directories.
py -3 tools/verify-source-lanes.py --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --run-directory local/YOUR-NEW-LANE-PROOF --regression-directory local/YOUR-NEW-RUNTIME-PROOF
```

Raw source assets and complete reports remain ignored and local. Public checkpoint
receipts contain counts, hashes, scope and remaining gaps. Actor initialization,
inheritance, automatic statistics and AI are unimplemented. Decoded skin data does
not establish evaluated poses, normalization rules, animation or dismemberment
behavior. No gameplay scenario is accepted by this source proof.


## Extended candidate proof for checkpoint 45

Checkpoint 44 keeps its original helper and historical receipts. The separate
`tools/build-integration-candidate.py` records clean committed source, checks,
native builds and executable identities in the integration worktree.
`tools/verify-integration-candidate.py` requires that build receipt, fresh private
proof/regression directories and an unchanged candidate before and after the run.
Its extended scope includes actor associations, classes, factions and placements;
three skin schemas including partitions and decoded source-forest ancestry; and
fresh live operand captures compared with independently exported native tuples
and saved engineering storage. Intended diagnostics distinguish the negative
cases from unrelated failures. The original deleted voice link remains explicit.

The runtime selector remains 43 with `--no-publish`; it names the repeated
regression surface. It does not relabel or overwrite the published checkpoint 43.
The public checkpoint-45 receipts identify the integrated implementation revision,
actual executed binaries, frozen inputs, raw receipt hash and limitations. Main
promotion adds metadata without changing the implementation proved by that run.
No original execution, evaluated skinning or gameplay scenario is accepted.
