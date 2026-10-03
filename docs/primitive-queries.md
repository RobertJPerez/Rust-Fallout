# Shared primitive query routing

The runtime now routes the verified NV `GetItemCount` entry IDs through one host
query: native command 4143 (`0x102F`) and CTDA function 47. Both require an
explicitly resolved live subject and one typed content-key argument. There is no
implicit player/caller fallback, float-encoded identity or successful unknown
entry. This is an engineering interface, not bytecode or original handler execution.

The common implementation returns an exact checked host quantity sum and a bounded
trace of contributing item identities. Campaign, source cohort, current revision
and clocks stay attached. A prepared request can be reevaluated after restoration
within the same campaign/cohort; it observes current state and retains no transient
handle. Other campaigns/cohorts, missing/deleted keys and uninitialized banks fail.

Both adapters preserve that common trace. Original numeric return conversion is
explicitly absent: no `u64` sum is silently treated as a verified original double
or signed count. Condition comparison, AND/OR grouping, original subject selection,
native argument extraction/coercion and execution remain separate unfinished work.

The actual executable descriptor requires a parent and one nonoptional ObjectID
parameter, type 50. Pinned xNVSE names that type inventory-object-or-form-list.
Form lists therefore fail with an explicit unverified-expansion error; an empty
lookup is not substituted for missing list behavior. Other arguments are resolved
host inputs; original argument admission has not been accepted.

```powershell
.\target\release\fallout.exe primitive-query-state `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order profiles/nv-inspection-order.json `
  --new-repository local/primitive-query-example `
  --output local/primitive-query-example.json
```

The probe retains the source-validated item engineering state and evaluates six
subject/item pairs through both entries. Twelve traces agree on all common data
and leave canonical state unchanged. `primitive-query-load-probe` repeats them
in a cold process with explicit `--query-inputs`. A batch shares a 65,536
contribution budget; the input file and array budgets match the item-query tools.

Eight runtime tests cover wide counts, typed arguments, unknown entries, absent
subjects/banks, missing/deleted keys, form-list rejection, current state changes,
restoration, changed campaigns/cohorts and contribution bounds. The checkpoint
also reruns complete item/persistence regressions and all 80,467 winning condition
word bindings against independent readers. All 640 executable descriptors remain
metadata evidence. The command and condition registries distinguish that coverage
from the single shared host query and from unimplemented original behavior.

The selected primary source declarations are pinned
[xNVSE parameter types](https://github.com/xNVSE/NVSE/blob/0ccd23ad885ddae533c1790a3fc56cd073e38de3/nvse/nvse/CommandTable.h)
lines 64–71 and the
[xEdit GetItemCount condition row](https://github.com/TES5Edit/TES5Edit/blob/9fb016884bec138ea6c7b872cec831537d464c3e/Core/wbDefinitionsFNV.pas)
line 199. Original handler presence does not expose its behavior. No upstream
implementation was copied or linked and no original handler ran.

Original initialization, inventory count/list semantics, coercion, scheduling,
VM execution and full mutable world persistence remain open. No condition truth
value or retail gameplay scenario is accepted by this checkpoint.
