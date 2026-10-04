# Explicit inventory and quest boot

`fallout route-boot` restores an existing Rust Fallout save, applies explicitly
selected inventory and quest initialization, and writes the result to a new
Rust Fallout save directory. This is an engineering command. It does not start
a new original game, infer activation rules, or prepare models for play.

```powershell
fallout route-boot `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order '.\local\nv-order.json' `
  --save-root '.\local\existing-native-save' `
  --request '.\local\route-boot-request.json' `
  --destination '.\local\new-route-save'
```

Use the exact load order and campaign that created the input native save. The
destination must be absent, its parent must already exist, and its ancestry must
consist of ordinary directories. A destination inside the installation or any
input, through a junction, or containing parent traversal is refused. The command
does not overwrite the input save. Ordinary Fallout saves cannot be used here.

The request is strict JSON with these fields:

| Field | Required value or meaning |
| --- | --- |
| `schema_version` | `1` |
| `intent` | `"engineering"` |
| `request_id` | A nonzero request identity |
| `restore_timeout_ms` | Between 1 and 30,000 milliseconds |
| `campaign` | The input campaign's 16-byte identity |
| `catalogue_sha256` | The exact input script catalogue identity, as lowercase SHA-256 |
| `input_snapshot_sha256` | The native input's stored snapshot hash |
| `input_revision` | The exact input state revision |
| `cell` | The selected source `FormKey` |
| `required_references` | Between 1 and 64 unique live and authored reference pairs |
| `actor_inventory` | Explicit actor inventory selection, or `null` |
| `quest_owner` | Explicit quest attachment initialization, or `null` |
| `require_faithful_simulation` | `false` for this engineering command |

At least one of `actor_inventory` and `quest_owner` must be supplied. Unknown
fields are errors. A `FormKey` carries `profile`, `origin_plugin`, and `local_id`;
use source identities already present in the input rather than inventing a live
reference. Preserve integer IDs and raw bit words exactly when creating JSON.

Each required reference supplies `reference`, `authored`, `enabled`, and
`require_scale`. Its authored record must be a resolved placed member of the
selected CELL. Its restored live state must match that authored identity, CELL,
and enable value; a requested scale must already be available.

An inventory selection supplies `actor`, `owner`, and `choices`. The owner must
be one of the required references and its unique resolved `NAME` base must be
the selected actor. Each choice supplies its physical `field_index`, exact
signed `source_count_claim`, explicit positive `host_count`, and complete item
`facts`. The source count is evidence; it is not automatically an inventory
count. There may be at most 64 choices. See the existing
[actor source documentation](actor-sources.md) and
[actor rules](actor-rules.md) for the item fact representation.

A quest selection supplies `quest` and `initialization`. Initialization supplies
its exact campaign, explicit context, and at most 128 local initializers using
the existing attachment boot request. Omitted local values retain the existing
uninitialized representation. This creates the selected source-attached owner;
it does not run a quest stage or synthesize original events.

On success, stdout contains a JSON report with the admitted source identities,
reference observations, inventory/quest results, and native publication receipt.
The command compares the complete resulting canonical state before writing and
loads the new repository strictly after writing. The report distinguishes the
stored input snapshot hash from its canonical encoding hash; those can differ
for a valid noncanonical input encoding.

Preserve the fresh destination if an error reports that publication already
happened or its stage may have written a current save. In particular, failed
stdout delivery does not undo a successful save: the error includes the actual
write receipt. Do not retry using the same destination.

Development09's authored acceptance covered four successful publications,
one successful publication followed by stdout failure, and 37 intended refusals.
It compared complete native bytes and canonical snapshots. The two reported
digests `actor_inventory.source_definition_sha256` and
`quest_owner.prepared_decoder_sha256` were transport observations, without an
independent expected value. Deadline admission was tested; deterministic expiry
and power-loss durability were not established by that matrix.

`model_assets_prepared`, `faithful_simulation_admitted`, and
`retail_parity_accepted` remain false. Rendering, collision, original activation,
playable routes, and original gameplay need their own acceptance work.

A [separate scene continuity proof](integration/team-v3-boot-render-save-01.json)
combined this boot command with the existing preview and native-save host. Four
fresh processes rendered an authored static source model before boot, published
a fresh native repository, rendered and awaited a real Saved receipt, then
loaded generation2 in a cold preview. Complete native bytes and canonical state
matched independent expectations. All three 1280 by 900 RGB captures matched,
with 23,871 strongly nonbackground pixels. The comparison establishes continuity
of the same authored scene; it is not an original-game pixel comparison. Actor
models remained an explicit unsupported omission, and playable movement, physics
readiness, controller playback and original gameplay were not accepted. The boot
command's own model preparation and faithful-simulation flags remain false.
