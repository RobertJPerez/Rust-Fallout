# Skyrim player movement source profile

`trace-movement-profile` exports a bounded, source-only chain from physical
`NPC_` Player records through `RNAM` to `RACE`, then from the race's movement
defaults and `MTYP` entries to physical `MOVT` records. It keeps every scanned
physical record with a matching source key, including overrides that omit the
`Player` editor ID. It does not pick a load-order winner or decide which
movement type the executable uses.

The race's `EDID` text is retained so a Player-to-race link can be read without
joining external editor-ID databases. This identifier is descriptive only and
does not determine which racial or movement override wins.

```powershell
.\target\debug\skyrim-prep.exe trace-movement-profile `
  --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' `
  --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim.esm' `
  --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Update.esm' `
  --output 'local\movement-profile.jsonl'
```

The Player selector requires an exact, NUL-terminated `EDID` value `Player`.
After finding one or more such physical rows, the trace also retains `NPC_`
rows with the same source key. A row selected only by that identity join is
marked `same-source-key-as-player-edid`, so a patch that omits its inherited
editor ID remains visible. If the supplied plugin set contains no matching
Player editor ID, the report is still emitted with zero Player candidates and
the command returns findings.

`NPC_ RNAM` is accepted only as an exact four-byte `RACE` FormID. For each
physical race row, `WKMV`, `RNMV`, `SWMV`, `FLMV`, `SNMV`, `SPMV`, and repeated
`MTYP` values are retained as links to `MOVT`. The trace keeps repeated
`MTYP` links and `SPED` override fields separate, with their original payload
offsets. It does not construct race entry objects from adjacency or infer an
override-selection rule.

`MOVT SPED` retains ten single-precision fields for record versions below 28
and eleven fields from version 28 onward. `MOVT INAM` retains its three
single-precision thresholds. `RACE SPED` retains its eleven fields, including
the final schema slot named `unknown`. Each recognized float field preserves
the whole-field hash and every slot's exact IEEE-754 bits; a JSON number is
included only for finite values. No angle-unit conversion, Creation-unit
conversion, speed multiplier, gravity, acceleration, jump rule, or locomotion
formula is inferred. Unsupported payload lengths remain malformed findings.

Ordinary FormIDs resolve through the referring plugin's declared masters.
Physical light-plugin records retain their validated file-local IDs, while
cross-file `FE` references remain profile-dependent. The output reports each
physical target-candidate count and never chooses an override. Fields outside
the movement adapter retain hashes, exact lengths, and bounded previews on
included `RACE`/`MOVT` rows; all scanned NPC, RACE, and MOVT field tags and
payload lengths also appear in the structural shape census. The shared
`fallout-data` visitor and digest implementation own framing and hashing.

The trace describes supplied plugin files, not active plugins or a verified
load order. Player race selection, sex-specific behavior, racial movement
overrides, actor-value modifiers, and runtime movement use still need separate
evidence. This command is preparation evidence, not Skyrim movement or gameplay
acceptance. Installed Special Edition and Anniversary executables can share
the same structural adapter when their actual record versions satisfy these
layouts; a marketing edition label alone is not a corpus-completeness check.
