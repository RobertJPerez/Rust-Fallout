# Source-bound item checks

Host item mutations can now validate content links against the immutable winning
header index. `World::add_source_item` and `replace_source_item_facts` complete
source checks before calling the existing atomic canonical transactions. A
missing, deleted, noncanonical or disallowed key fails without changing state.

The source index is shared with foreign-context lookup. It is rebuilt from whole
source hashes and winning headers, bound to the script catalogue cohort, and
excluded from snapshots. Item checks perform ordered lookups without scanning
plugins again or reading deferred record bodies. Whole-record overrides,
tombstones and master-relative namespaces determine the queried header.

The caller supplies an explicit `source_items::Policy`: allowed four-byte record
kinds for base items, actor/faction ownership, ammunition and modifications.
Every used role needs a rule; missing rules do not allow everything. Policies
reject duplicate roles/kinds and invalid tags, allow at most five roles and 64
kinds per role, and have an order-independent SHA-256 identity. These are explicit
host rules. No built-in rule is presented as original runtime admission behavior.

Validation also checks canonical links and per-item budgets before allocating
diagnostic rows. Unknown facts remain absent. Repeated modification keys retain
their order. Successful proofs identify campaign, pre-mutation revision, cohort,
policy and every checked key/kind/flag occurrence. A proof is diagnostic data,
not authorization to skip future validation after state or policy changes.

```powershell
.\target\release\fallout.exe source-item-state `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --new-repository local/source-item-example `
  --output local/source-item-example.json
```

The probe discloses its policy derived from three original source identities and
explicit engineering facts. It rejects a boundedly searched missing key, an
actual deleted winner and a deliberately disallowed QUST base rule. It adds nine
host units and replaces their base through the validated API, then saves and
repeats all observations after restore. `source-item-load-probe` repeats this in
a fresh process using `--query-inputs`; exactly three disclosed item keys are
required. Diagnostic item/form/query contributions share a 65,536-entry budget.

Eight runtime tests cover every role, duplicate links, unknown facts, policy
bounds/order, changed cohorts, cold canonical restore, wrong kinds, tombstones,
overrides, namespaces and failure atomicity. The installed probe independently
checks every admitted form's kind/flags against original base-inventory bindings
and the full source-header classification digest. Independent save-container
checks and complete prior checkpoint regressions are separate evidence.

Raw host-state APIs remain available for explicit engineering state. Ordinary
snapshot restoration checks canonical syntax and persistent links; source-policy
validation is a separate caller-selected step. Header validity does not establish
body/asset residency, item initialization, original ownership/equipment rules,
stacking, callbacks or eligibility. Original item admission and all retail
gameplay acceptance remain unfinished.
