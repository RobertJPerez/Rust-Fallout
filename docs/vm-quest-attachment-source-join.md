# Quest attachment source joins

Static foreign declaration lookup validates the quest attachment's complete
source-file hash against the script catalogue's retained source receipt. The
source name, winning record offset and flags must also match. A missing or
different source receipt produces the existing `QuestWinnerMismatch` before
selecting a target script or declaration.

This closes a demonstrated mixed-snapshot join: a quest's `SCRI` can change in
a separate plugin while its record header and the caller and both possible
target script handles remain identical. Joining that attachment with the old
catalogue previously selected a declaration from the changed quest source.

The guard uses existing immutable source metadata. It adds no public type,
status, command flag, runtime bank, event policy or save format. Independently
loaded equal source sets and unrelated plugin/order changes still resolve
when the quest and target definition sources match. Live script instances and
values continue to use the canonical runtime's separate context checks.

The existing `quest-scripts` inspector remains the static consumer. Private
authored three-plugin fixtures reproduce the stale selection in both
directions, retain exact source bytes and show rejection after the fix.
Focused tests preserve equal-source and unrelated-source cases. Fresh
consistent-source inspection is compared with the independent native quest
reader; raw `SCRI` words and source hashes are independently read separately.
Cross-set rejection is checked through the public declaration API. These
checks establish source association and leave original runtime timing,
numeric rules, native behavior and live values unmeasured.
