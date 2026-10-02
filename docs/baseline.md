# Local New Vegas baseline

Observed on 2026-10-02 at `G:\SteamLibrary\steamapps\common\Fallout New Vegas`.
The installation was opened read-only. No executable, plugin, archive, configuration,
load-order file, or save was modified. The original game was not launched.

| Item | Observation |
| --- | --- |
| Executable file/product version | 1.4.0.525 |
| Executable bytes | 16,549,704 |
| Executable SHA-256 | `3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d` |
| FalloutNV.esm SHA-256 | `50991d36804b7d1e70df1afd7471b72f0e29d1b456ee2516a9717c002564e7c1` |
| Whole-install content fingerprint | `b1e54397ea2e900913c33feecbdb27b1bc33de95241cd5e0bcdd0250e79de007` |
| Files / bytes hashed | 464 / 9,907,238,722 |
| Official content | Base game, four story DLCs, Gun Runners' Arsenal, four preorder packs |
| Top-level plugin/archive census | 10 ESM, 0 ESP, 21 BSA |
| CPU | Intel Core i9-9900K, 8 cores / 16 threads |
| GPU | NVIDIA GeForce RTX 5090, driver 32.0.15.9186 |
| RAM | 42,881,798,144 bytes |

`local/baseline.json` contains every relative filename, size, and SHA-256. Its content
fingerprint excludes absolute installation location and timestamps. The checked-in
[profile manifest](../profiles/manifest.json) carries the executable/plugin/archive digests.
The [environment report](../reports/environment.json) records the observed machine.

`%LOCALAPPDATA%\FalloutNV\plugins.txt` and `NVDLCList.txt` were empty. No effective
game INIs were found under the inspected Documents game directory. The installation's
default INI says English and `bInvalidateOlderFiles=1`; those are defaults, not captured
effective settings. Language, difficulty, bindings, effective INIs, active order, and
retail oracle traces remain unverified. The inspection order under `profiles/` is
solely a reproducible dependency-valid input for our tools.

Strict content acceptance is still open because of the three
[format exceptions](format-exceptions.md), missing semantic decoders, and absent
retail behavior captures. No FO3, TTW, FO4, FO76, or Starfield baseline was collected;
their profile manifests say so instead of inferring ownership or runtime compatibility.
