# Source-native inventory assignment (V3-VM-31)

The saved host can perform one explicit engineering assignment from a compiled
GetItemCount expression into an existing own numeric local. It uses the cached
source plan, native occurrence admission, inventory query, and opaque event
stage already used by other consumers. No native implementation or parser is
added. Original numeric returns, coercion and event lifetime remain unverified.

native_assignment::stage takes the current World, PreparedSources, Content,
Selection { sequence, inputs, intent }, and Limits. Only
EngineeringExactCountToNumber admits effects. Faithful returns typed
unsupported. A staged proposal exposes diagnostic trace and existing opaque
changes; its consuming commit uses canonical validation and the atomic
assignment/acknowledgment boundary.

The journal head must contain exactly BEGIN, one own numeric assignment and END.
Its expression contains one command, optionally preceded by one reference
prefix. The command, token/instruction/argument byte ranges, destination
declaration, write binding, prefix binding and argument binding must agree.
Additional statements, operators and reads refuse. The sole supported command
is GetItemCount (0x102f). The existing native resolver supplies the exact caller
and one source reference-table argument. Explicit subject, explicit player,
source caller, instance owner and event context remain separate.

The resolved query reads canonical inventory only. Form-list expansion,
unsupported caller/argument forms, absent subject inventory and unknown native
commands refuse. Integer normalization constructs a binary64 word directly
from the unsigned count. Any nonzero discarded bits refuse; no floating-point
cast or decimal parsing rounds a result. With current world item ceilings and
the default contribution cap, an inexact total cannot arise through this CLI.
Independent arithmetic boundary tests cover that guard without claiming an
unreachable saved-world probe.

All fourteen runtime caps are explicit in the saved request:

| Request field | Default ceiling |
| --- | ---: |
| maximum_source_instructions | 4096 |
| maximum_calls | 1 |
| maximum_argument_bytes | 1024 |
| maximum_operand_uses | 3 |
| maximum_statement_bytes | 65539 |
| maximum_trace_source_bytes | 1048576 |
| maximum_trace_rows | 65536 |
| maximum_trace_variable_bytes | 1048576 |
| maximum_trace_binding_uses | 262144 |
| maximum_query_variable_bytes | 1024 |
| maximum_stage_variable_bytes | 3145728 |
| maximum_inventory_visits | 65536 |
| maximum_contributions | 4096 |
| maximum_trace_bytes | 2097152 |

Borrowed caller/argument names are charged before bounded internal native
admission copies them. A conservative reservation admits twice the observation
variable bytes, seven copies of the largest borrowed name, source/decoder
digests and 1024 bytes before query or stage retention. The inventory preflight
and existing query each scan the bank: both passes count cumulatively against
the visit cap, including unrelated items. Contributions have their own cap.
Compact trace bytes are admitted before the stage is created. These are logical
work and retained-data limits, not a hard real-time or peak-heap guarantee.

The actual CLI entry is event-operands --snapshot-native-assignment-request.
It requires snapshot input/output, an exact owner, a nonzero journal sequence,
and required nullable supplied_subject and explicit_player. Intent is a
string: faithful or engineering_exact_count_to_number. Request schema 1
rejects unknown, duplicate and missing fields; the request ceiling is 16 KiB.
All fourteen caps, four whole-definition preparation caps and result/report
caps are required and may only reduce existing defaults. Preparation caps are
maximum_prepared_instructions, maximum_prepared_operand_uses,
maximum_prepared_tokens, and maximum_prepared_record_bytes. The current
snapshot ceiling is 64 MiB and report ceiling is 8 MiB including its newline.

Source metadata and catalogue are inspected from the supplied authored install.
The saved input restores into a private world. After canonical commit, the
complete encoded result is decoded and cold-restored for equality. The complete
report is admitted before creating a fresh result. Existing protected-tree,
fresh-output and Windows case-alias checks are reused. Unsupported requests
publish no result and exit unsuccessfully. Filesystem interruptions remain
incomplete; result and report publication is not a multi-file transaction.

The independent authored baseline has split contributions (1,4), (2,13)
and (3,3): count 20, word 0x4034000000000000. Its source command is SCDA
22..32, argument 27..32, statement 10..32 and destination index 90. Four bank
rows, including an unrelated form, charge eight visits. The expected snapshot
changes only that destination, journal head and revision. Entire cold equality
preserves both inventories, lot facts/NaN payloads/opaque extras, other locals,
contexts, reference state, allocator counters, clocks and the next event.

Source layout is grounded in pinned NVSE argument/expression source already
recorded with VM30, plus existing compiled source plans and resolver/query
contracts. It does not establish Original return precision or assignment
semantics. Authored executable metadata is inspected, never launched as Original.
Engineering acceptance does not establish route, checkpoint or gameplay parity.
