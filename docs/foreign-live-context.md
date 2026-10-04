# Foreign locals through live script instances

`fallout-runtime` now resolves a compiled foreign-local request through the
source instance's one-based reference table, the target owner and that owner's
current live script instance. The target's declaration bank supplies the local
kind and value. A quest's authored SCRI, a placed reference's base script and an
equally numbered local in the caller do not supply a missing live bank.

```powershell
.\target\release\fallout.exe foreign-context `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --new-repository local/foreign-live-example `
  --output local/foreign-live-example.json

$probe = Get-Content -Raw local/foreign-live-example.json | ConvertFrom-Json
.\target\release\fallout.exe foreign-load-probe `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --repository local/foreign-live-example `
  --player-id $probe.engineering_inputs.player_reference `
  --output local/foreign-live-cold.json
```

These are engineering commands. They explicitly supply source instances, quest
lists from authored SCRI definitions and placed lists from one disclosed template.
They assign source numeric values of 1, target numeric values of 10 and null
reference values. The template is not an observation of the original placed
script. The player identity is an explicit harness input; player-role persistence
belongs to the future player component. No bytecode is executed.

## Resolution contract

An immutable header index binds all source lengths/digests and every winning
header to the loaded catalogue. It classifies QUST and the supported authored
REFR/ACHR/ACRE/PGRE/PMIS/PBEA kinds, retaining other and deleted forms. This derived
index is rebuilt after load and is absent from save state. Building it is bounded;
lookup uses ordered maps without rescanning source files or bytecode per request.
Header classification does not validate deferred payloads or admit the known
malformed LAND record.

Static quest references select `Owner::Quest`. Static placed references need a
registered canonical reference identity before selecting `Owner::Placed`. SCRV
contexts read the source's typed reference local; null and uninitialized values
fail explicitly. Live created references and an explicitly bound player select
their placed owner. Authored live origins must have a supported, nondeleted
placed kind; an actor base or quest identity cannot masquerade as a live reference.

The owner map supplies the current script instance. Replacing that instance
changes the definition and bank used by subsequent lookups. Missing event lists,
absent indices, unsupported kinds and dangling bindings remain failures. Reads
preserve exact numeric bits and typed references. Foreign writes resolve again
under an exclusive world borrow and use the same atomic assignment validation
as ordinary local writes. A returned target records the revision at lookup,
before any write; it is diagnostic evidence, not a cached mutation capability.

Persistent owner links survive native restoration. Old source slot handles fail;
the host rebuilds transient handles from persistent instance IDs. Constructor
defaults, list creation/replacement timing, residency and event scheduling remain
unmeasured. Registering a reference is an explicit host input, not proof that the
original engine would keep its event list alive.

## Evidence and scope

| Installed-content engineering probe | Result |
| --- | --- |
| Compiled units and foreign requests | 14,476 units / 25,549 requests |
| Without supplied target lists | 20,542 missing quest lists / 5,007 unregistered placed references |
| Explicit host inputs | 7,392 source instances / 465 quest lists / 438 placed lists / one player list |
| With those test lists | 21,872 resolved values / 3,677 absent indices in the chosen test banks |
| Canonical state | 8,296 instances; 4,243,652-byte snapshot |
| Restoration | Every lookup result agrees after restore; pre-restore handles fail |

Missing indices in the chosen placed template are expected engineering outcomes,
not defects repaired by guessing declarations. Original source and target values
were not captured. Counts do not establish gameplay command coverage.

Nine tests cover current live definitions, absent lists, typed dynamic contexts,
explicit player bindings, exact values, atomic writes, changed sources, restore,
DLC master namespaces and winning target overrides. An original offline C++
reader directly rebuilds all 628,464 winning header classifications: 640 quests,
427,387 placed forms, 200,437 other forms and 68 tombstones. Its classification
digest agrees exactly with Rust.

The checkpoint driver freshly repeats native-save/schema checks, independent
quest attachments and all operand/header/table/native/expression comparisons.
Each foreign request's source key, encoded offset, role, context index and local
index agrees with the independent static scan. A separate process restores the
native file and repeats every lookup, checking a digest of all full outcomes.
The original C++ container reader also verifies this larger state file.
None of those source comparisons measures original live behavior.

## Explicit owner lifecycle regression

`save_foreign_lifecycle` exercises the existing foreign resolver, canonical
removal guards and native save worker with authored compiled static SCRO and
dynamic SCRV reads of one placed owner's bank. No statements are executed.
The host explicitly supplies the placed reference, bank, exact numeric bits,
pending events and an inventory item linked to that script instance.

Removing the bank with its event still pending fails without changing the
snapshot. After an explicit FIFO head acknowledgment, the item script link still
blocks removal. The host explicitly detaches that link before removal succeeds.
The registered reference, source's live-reference local, inventory ownership and
remaining source event survive unload. Both foreign reads then report a missing
live event list; the authored base script does not supply a replacement bank.

An explicit new instance for the same placed owner receives a new persistent
instance ID and a different supplied definition/bank. Both reads select that
current bank and preserve its supplied numeric bits. The retired instance ID and
old slot handle cannot name the new bank. This describes explicit host actions,
not original unload timing, automatic script attachment or numeric conversion.

The bounded worker captures live, unloaded and reattached boundaries. Its current
and previous slots retain complete snapshots despite later host mutation. Three
fresh processes restore the entire expected state, rebuild the foreign index,
repeat both reads and preserve container files. The existing `foreign-load-probe`
can consume each retained phase repository with the explicit player identity;
no new CLI option or save field is required.

```powershell
py -3 G:\Rust-Fallout\tools\team-v2-focused.py --lane runtime -- powershell.exe -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked --jobs 2 -p fallout-runtime --test save_foreign_lifecycle --test foreign --test save_worker --test items
```

Set `FALLOUT_FOREIGN_LIFECYCLE_EVIDENCE` to a new private directory to retain the
authored plugin/load order, phase snapshots, native containers and cold receipts
under a fresh `authored` subdirectory. Existing evidence is never overwritten.

The schema-3 migration regression starts with a populated item bank, an explicit
empty bank and an uninitialized bank. Explicit `migrate_v3` preserves item owner,
script instance, ownership, condition bits and opaque bytes alongside both
compiled foreign read sites and their pending contexts. Reference pose and enable
state remain unavailable. Old script and item handles cannot name the restored
state; the host reacquires handles from persistent IDs.

The existing worker publishes pre- and post-mutation snapshots. Three fresh
processes compare complete current/previous state, repeat static and dynamic
foreign reads, and query the distinct inventory banks. Another campaign, an
added plugin, and a body-only ACTI script-link change refuse without modifying
the original repository. The body-only case preserves every winning header,
compiled script record and declaration. Exact version handles still change
because they include the whole source hash; the resolver and loader refuse them.
The original schema-3 file and authored input remain unchanged. Set
`FALLOUT_FOREIGN_MIGRATION_V3_EVIDENCE` to a new private directory when running
`schema_three_foreign_migration_retains_existing_item_links_and_refuses_other_identity`
to retain this separate engineering proof.

The selected pinned references are
[ResolveExternalVar and EventListFromForm/GetParentScript](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameAPI.cpp),
lines 919-936 and 1964-2001,
[RefVariable::Resolve](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameScript.cpp),
lines 323-334, and the
[FormType declarations](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/GameForms.h),
lines 1-140. They describe selection/storage relationships; executable calls and
original timing are not standalone implementations. No upstream implementation
was copied or linked. Original numeric coercion, command effects, bytecode
evaluation and retail differential traces remain future work.
