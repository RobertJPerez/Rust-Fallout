# Game profiles

[manifest.json](../profiles/manifest.json) is the machine-readable profile catalog.
Only `nv-original` has a local corpus fingerprint. The other entries preserve the
brief's scope and declare their missing baselines and unsupported scenarios explicitly.
An entry in the catalog is not an implementation of its formats or rules.

| Profile | Current evidence | Rules boundary |
| --- | --- | --- |
| `nv-original` | Retail 1.4.0.525 files, official DLC corpus, headless tools | Original NV semantics; still no gameplay runtime |
| `fo3-original` | No baseline collected | Separate original FO3 behavior |
| `ttw-compatible` | No TTW installation baselined | Pinned TTW's conversions, quests, travel, and required extender providers |
| `fo4-original` | No baseline collected | FO4 record/animation/material/Papyrus behavior |
| `fo76-research` | No baseline collected | Versioned client evidence and explicit unknown server responsibilities |
| `starfield-probe` | No baseline collected | Bounded format/behavior probes only |
| `unified-crossover` | No travel policy or runtime | Intentional crossover rules, separate from original-game parity |

`FormKey` and asset cache identities include the profile today. Tests verify that
otherwise identical plugin/local IDs and asset inputs remain separate across profiles.
No save loader exists, so R3-01's cross-profile save-rejection acceptance test remains
open. The eventual save envelope must bind a versioned profile manifest and compatible
content identity before any state is restored.

The explicit order in `nv-inspection-order.json` is used only for tool comparisons.
It does not edit or certify the original game's active order. Effective runtime INIs,
language, difficulty, bindings, and behavior captures are still absent.

The NV tool profile now includes typed cell dependency inspection and NIF/KF
container inventories, with an independent nifly comparison. Those scenarios remain
headless inspection; no cell activation, rendering, movement, or collision is accepted.

R3-02 now has deterministic source-preparation plans, with source/master digests and
explicit unsupported inputs. R3-03 has structural definition relocation and two-pass
SCRO links; other typed links and TTW relocation remain open. R3-04 has verified,
resumable single-asset publication; full transformation scheduling, cancellation, and
journaling remain open. None of these partial results implies completed conversion.
