# Implementation checkpoint — 2026-10-02

The executable is `target/release/fallout.exe`. It runs independent Rust content tools
against the authorized installation. There is no playable scene or gameplay runtime.
The original installation was not modified. M0's headless/bootstrap/source work is
present; the Bevy application spike and retail profile captures remain open. M1 is
in progress. M2 onward and campaign acceptance have not been reached.

## Actual commands and results

Commands below were run from `G:\Rust-Fallout`; the Cargo wrapper invokes the local
Rust 1.99.0 MSVC toolchain. Full source digests and corpus counters are in
[corpus.json](corpus.json). Raw local reports remain under `local/`.

| Command or run | Actual result |
| --- | --- |
| `tools/cargo.ps1 build --release --locked -p fallout-cli` | Exit 0; release executable built |
| `tools/cargo.ps1 test --workspace --locked` | Exit 0; 9 unit tests and 12 integration tests pass |
| `tools/cargo.ps1 clippy --workspace --all-targets --locked -- -D warnings` | Exit 0 |
| `tools/cargo.ps1 fmt --all` | Formatting applied; final check through `tools/check.ps1` |
| `fallout baseline --install <NV> --output local/baseline.json` | Exit 0; all 464 files hashed, no required official plugin missing |
| `fallout census --install <NV>` | Exit 1 at vanilla LAND checksum mismatch |
| `fallout census --install <NV> --inspect-checksum-mismatches --output local/census-with-scripts.json` | Exit 1 after writing a complete diagnostic census with one integrity issue |
| `fallout resolve --install <NV> --load-order local/inspection-load-order.json --inspect-checksum-mismatches --output local/resolution-final.json` | Exit 1 due to the same integrity issue; zero missing SCRO targets after explicit player-binding classification |
| `fallout plan --install <NV> --load-order local/inspection-load-order.json --inspect-checksum-mismatches --output local/import-plan.json` | Exit 1 with taint retained; 31 source-preparation jobs, 275 unsupported loose inputs, 158 non-Data files left untouched |
| `archive-compare <each BSA> --all` across all 21 archives | 20 exit 0; Misc archive exits 1 with two preserved decode failures |
| Separate `plugin-oracle <NV>/Data` | Exit 0; all ten plugin counts and masters agree with the independent parser |
| `fallout asset <Caravan BSA> <caravanshotgunpreorder.nif> --cache-root local/cache` twice | Both exit 0; first publication then verified reuse of the same 177,914 bytes |
| `py -3 tools/report.py` | Generated public count/condition/profile/requirements summaries from local evidence |
| `tools/check.ps1` | Exit 0; final formatting, all 21 tests, and strict Clippy checks pass |
| README `inspect` example, CaravanPack `001735DD` | Exit 0; header and bounded subrecord previews written to `local/inspect-caravan.json` |
| Reuse an existing `--output` path | Exit 1; original report SHA-256 unchanged |

The cached NIF has SHA-256
`6f49721b83771c56b3b3d95647b5be4da9983a59bf082f3f8f2c6be9f9250186`.
No NIF geometry, collision, texture, or animation semantics were evaluated by that test.

## Evidence interpretation

629,788 records exclude ten TES4 file headers. 628,464 stable definition keys and
780 overridden keys describe structural resolution, not all game-specific merging.
49,074 SCRO occurrences divide into 44,508 record links and 4,566 engine player
dependencies. These are reference occurrences, not unique commands or running scripts.
14,514 SCDA fields hold 1,966,273 compiled bytes; 80,627 CTDA fields reference 148
distinct function IDs. Command/event decoding remains unknown.

Archive readers compared every member path/hash and 182,175 decoded payloads, totaling
10,860,465,525 decoded bytes. The other two entries disagree on truncated-stream
handling. The base plugin's LAND mismatch also remains unresolved for runtime use.
See [the exact exceptions](../docs/format-exceptions.md). Counts from a diagnostic
recovery do not imply trusted or accepted world content.

Synthetic tests cover bounds, malformed streams, extended fields, source offsets,
taint rejection, explicit overrides, master failures, cyclic record links, canonical
identity collisions, profile separation, deterministic preparation IDs, source isolation,
and interrupted cache publication. They use original fixtures and do not measure retail
AI, script scheduling, rendering, animation, physics, UI, audio, quests, or saves.

The deterministic mutation sweep is not a sustained fuzzing campaign. Cache interruption
is injected inside the process; OS termination during all transformation stages remains
an open acceptance test. No frame times, visual parity numbers, or campaign completion
percentages were inferred from these results.

## Remaining acceptance checks

Effective retail settings/order and an isolated oracle scenario still need capture.
The game executable was not launched, and xEdit per-record exports have not been run.
The user has already authorized local work; no additional access request is needed.
Archive/loose precedence, typed field and asset dependencies, script bytecode, real
interior presentation/collision, simulation, saving, and the remaining full campaign
milestones are unfinished implementation work. Other-game profiles are catalog entries,
not runtime adapters.

There is no verified engine commit for this initial working tree. Ledger revisions
remain null. [checkpoint-01-source-snapshot.json](checkpoint-01-source-snapshot.json) fingerprints the uncommitted
source files and lockfiles. [NEXT_STEPS.md](../NEXT_STEPS.md) names the exact next dependency gates
without treating a successful loader as a finished Fallout recreation.
