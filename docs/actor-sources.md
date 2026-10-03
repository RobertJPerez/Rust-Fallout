# Immutable actor source fields

ACT-01 adds `actors::Catalogue::load(&inventory::Catalogue, actors::Limits)`.
It decodes the remaining authored NPC_/CREA scalar fields over the existing
inventory production loader. The actor catalogue borrows the exact inventory
definitions, source receipts, retained records and winning metadata digest. It
does not rebuild CNTO/COED/TPLT bindings or join unrelated inputs by record count.
The inventory definition remains available through `inventory_definition()`;
raw unknown and unselected fields remain available through `record()`.

This is source decoding. No actor is initialized, no template is inherited and
no auto-calculated statistic, level conversion, combat rule or AI behavior is
implemented or accepted. Source hashes and a winning header digest together bind
the cohort; a header digest alone does not prove equality of record payloads.

## Admitted disk layouts

| Field | Authored values | Shape |
| --- | --- | --- |
| NPC_/CREA ACBS | Fatigue, barter gold, level word, PC-level-mult flag, calculation bounds, speed multiplier, karma float bits and signed disposition | 24 bytes |
| NPC_ DATA | Signed 32-bit base health, seven attribute bytes, unchanged unused trailing bytes | At least 11 bytes; the pinned schema explicitly declares a variable trailing byte array |
| NPC_ DNAM | Fourteen skill values and fourteen skill-offset bytes, both unsigned | 28 bytes |
| CREA DATA | Type, combat/magic/stealth bytes, signed 16-bit health/damage, two unused bytes and seven attributes | 17 bytes |

The attribute order is Strength, Perception, Endurance, Charisma, Intelligence,
Agility, Luck. Skill order is Barter, Big Guns, Energy Weapons, Explosives,
Lockpick, Medicine, Melee Weapons, Repair, Science, Guns, Sneak, Speech, Survival,
Unarmed. These are disk labels and ordering, not effective runtime actor values.
ACBS flags and template flags are already retained by the inventory catalogue;
the actor projection does not replace those existing fields. The level word is
never divided, clamped or used to generate a runtime level.

NPC record versions 14 and 15 and creature versions 9, 11, 13, 14 and 15 were
independently observed in this installation and are explicitly admitted. Other
versions return an unsupported error, even when lengths happen to match. This
does not imply a format change between every other version: those variants have
not been verified here. Every current NPC DATA field is 11 bytes. Authored tests
exercise the schema-declared unused tail, including a 25-byte field, without
inventing a legacy-version threshold or claiming retail behavior for that tail.

Every physical field retains its kind, decoded offset, extent, SHA-256 and order.
Duplicate selected fields remain separate occurrences with findings. Missing ACBS
and DATA produce findings without fabricated defaults. DNAM absence remains
absence; the pinned schema does not mark it required. Creature DNAM and other
unselected fields remain opaque. Integer signs and all karma bit patterns,
including NaNs, are retained exactly. Record, byte and field budgets bound the
decoder. Truncations, unsupported known lengths, tainted input and direct decoding
of deleted records are rejected with source context.

Deleted inventory winners remain tombstones, with empty scalar fields and no
fallback to older definitions. Their `record_version` is explicitly null because
the inventory catalogue intentionally retains no decoded record for tombstones.
Their original header remains bound through the winning metadata digest, source
plugin hash, file offset and flags; no version is inferred from a predecessor.

## Reference pins

The disk reference is [xEdit at
9fb016884bec138ea6c7b872cec831537d464c3e](https://github.com/TES5Edit/TES5Edit/tree/9fb016884bec138ea6c7b872cec831537d464c3e).
MPL-2.0 notices were inspected in the existing project audit. This slice consults
layout facts and authors its own Rust/C++ code; it imports no Pascal implementation
and does not execute the Delphi application.

| Read-only reference | SHA-256 | Selected lines |
| --- | --- | --- |
| Core/wbDefinitionsFNV.pas | `89b1420415b858e004429e89a828348a1df71f5c8b199d4ea66e02f0ca41b012` | CREA 4276–4423; NPC_ 6798–6955; NPC editor fixup 2478–2493 |
| Core/wbDefinitionsCommon.pas | `e616b6546f6df74d88ec98bb870906db380c985963d011e4931c5726682b7d26` | Level union decider 5704–5716; editor level-mult clamp 923–936 |

The schema's editor level-mult clamp and NPC NAM5 fixup are not disk values and
are never applied. The primary agent owns any corresponding sources.lock.json
update after integration.

## Verification

The private ACT-01 development comparison used the ten plugins in
`profiles/nv-inspection-order.json`. Its source receipts and complete winning
metadata matched the separate C++ reader. That reader opens the original plugins
itself, resolves masters and winners, and decompresses selected original payloads
using the unchanged authored `oracle-common` reader. It never consumes Rust-decoded
records. C++ is confined to this offline comparison tool.

| Compared source facts | Observed result |
| --- | --- |
| NPC_/CREA winners | 4,220 / 2,235; 6,455 total |
| Complete physical actor fields | 204,929 equal, including opaque hashes and offsets |
| Selected scalar occurrences | 17,130 equal |
| Decoded actor body bytes | 5,337,465 |
| Selected scalar duplicates or missing required fields | Zero in this cohort |
| Index-cache phases | Ten cold misses; ten warm hits; ten hits after swapping the independent final two content packs |
| Authored direct-reader comparison | Five winners, including one tombstone, nine physical fields, eight scalars and five retained findings; cold/warm equality |
| Deliberately altered oracle scalar | Rust comparison rejects a changed fatigue word |
| Unsupported NPC version 16 / 18-byte CREA DATA | Both Rust and the offline reader reject each case |

Raw worker evidence stays local:
`G:\Rust-Fallout-worktrees\actors\local\act01-scalars-20261003-03`,
`local\act01-authored-comparison-20261003`, and
`local\act01-authored-inputs-20261003\negative-results-02`.
The private worker-comparison files pin binary/report hashes and actual command
arguments. They are not integrated checkpoint receipts. Earlier failed wrapper
runs remain in separate local directories and are not counted as completed runs.

Eight focused Rust tests pass. Formatting and all-target Clippy for fallout-data
and fallout-cli pass with warnings denied. Tests cover exact signed/float/byte
storage, legacy tails, field occurrences, missing values, version dispatch,
truncation, extended lengths, budgets, source lifetime, namespaces, overrides,
tombstones, actual cold/warm cache reuse and unrelated plugin reordering.

Run the inspector from the worker worktree after building its private target:

```powershell
$env:CARGO_TARGET_DIR = 'G:\Rust-Fallout-worktrees\actors\target'
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked -p fallout-data --test actors
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 clippy --locked -p fallout-data -p fallout-cli --all-targets -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 build --locked -p fallout-cli --bin fallout
.\target\debug\fallout.exe actor-sources --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles/nv-inspection-order.json --output local/actors-new.json
```

Build the native reader with `tools/actor-oracle/build.ps1 -BuildDirectory` pointing
to a private local directory. `tools/actor-oracle/compare.ps1` accepts explicit
`-Fallout`, `-Oracle`, `-Install`, `-LoadOrder` and a new `-RunDirectory`. It creates
its own index cache and compares cold, warm and reordered projections through the
Rust inspector's `--compare-oracle` option. Reordering requires independent final
plugins; use `-SkipReordered` for a dependent fixture. `-AllowSourceFindings` is
only for authored fixtures expected to retain findings and return diagnostic exit
1. The original cohort returns exit 0 in every phase. This tooling writes no
installation files and does not claim original-game behavior or gameplay parity.

ACT-02 is the next dependency: ordered authored actor associations, with exact
source cohort checks before joining a store. Runtime inheritance, initialization
and original gameplay measurements remain with the primary integration lane.
