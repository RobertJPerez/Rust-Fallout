# Source-bound script preparation

`obscript::definition_plan::prepare` selects an immutable winning unit through
its exact `loaded_scripts::Handle`. It combines freshly decoded delimiter plans,
complete expression plans and encoded operand associations from that same unit's
owning tables. The result borrows the checked source and winning content cohort.
It does not grant VM execution permission.

An equal compiled-body hash is insufficient to identify a definition. Embedded
units can contain identical SCDA while using different reference tables. A
prepared plan therefore keeps its complete handle, including the record key,
SCHR decoded offset and source-version hash. The tests exercise that distinction
with two embedded units which bind the same encoded index to different forms.

Preparation rejects missing/changed handles, absent SCDA fields, source metadata
findings, incomplete delimiter/expression structures, uninterpreted expression
tails, missing encoded table entries and native operand decode findings. It
returns no partial plan. Source metadata findings precede the absent-body check;
an inconsistent empty unit remains visible as a source finding.

The plan exposes immutable source, instructions, event groups, expression
statements and table associations. Expression lookup uses the exact instruction
index; event lookup uses the exact SCDA begin-header byte offset. Event IDs alone
do not uniquely select an event block. These lookups take O(log n) time and do
not choose an executable successor or invent an event context.

Operators and command signatures must come from verified metadata for the
selected profile. The CLI obtains them from the fingerprinted original
executable through the existing read-only descriptor reader. The data API keeps
that caller-supplied metadata separate from file paths and platform addresses.
The selected structural model and owning-table formats inherit the already
documented source scopes in [expression plans](expression-plans.md),
[control-flow structure](control-flow-structure.md) and
[operand associations](operand-bindings.md). No new upstream implementation is
copied or linked.

Source associations still contain deferred foreign locals and dynamic reference
dependencies. Their encoded indices can be well formed while their live owners
or values remain unavailable. Preparation does not initialize locals, resolve a
live default subject, enforce native form domains, decode event arguments,
dispatch an event, convert a numeric value or call a native handler. Those gates
remain separate. An absent SCDA field is reported as a source observation by the
corpus inspector; the strict data API returns `MissingBody` for that unit.

Per-definition defaults bound SCDA framing and conditional depth using the
control planner, each expression using the expression planner, and the combined
definition to 262,144 expression statements, one million tokens, one million
nodes and 262,144 encoded uses. The CLI also bounds the whole export to 65,536
compiled bodies, 66 MiB, 262,144 prepared expressions, two million tokens/nodes
and one million uses. Aggregate limits return errors before a report is emitted.

The installed-data development comparison checks all 80,627 winning units and
14,476 compiled bodies. It prepares 14,420 source structures containing 52,038
expressions, 145,136 tokens, 129,626 nodes and 156,273 encoded uses. The inspector
retains 53 control findings, three expression findings and three table-count
findings in units without SCDA. A further 66,148 units have no compiled field
and no metadata finding. Authored-source counts from earlier censuses differ
because overridden definitions remain in the authored corpus.

The independent comparison joins every winning handle/version/owner to the
freshly checked catalogue, every compiled body to independently decoded
control/expression reports, and every prepared binding digest/count to the
freshly checked physical source unit. Record file offsets and SCHR decoded
offsets are kept distinct. Full body hashes additionally bind the export order.
The new exporter is Rust-owned; the reused C++ tools independently inspect its
bytes, while the existing independent catalogue and source-table checks bind
the extraction and owning-record provenance.

```powershell
.\target\release\fallout.exe source-plans --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles\nv-inspection-order.json --comparison-bundle local\YOUR-NEW-BUNDLE.bin --output local\YOUR-NEW-REPORT.json
```

The command returns 1 when unresolved source findings are present. Every reported
finding retains a handle and source version. All output files require new paths
outside the installation. Compiled payloads remain in ignored local evidence;
public receipts contain metadata, identities, counts and hashes. Final workspace,
source-cohort and installation verification belongs to the checkpoint receipt.
Original script scheduling, arithmetic, branch rules, native effects, campaign
behavior and gameplay acceptance remain unfinished.
