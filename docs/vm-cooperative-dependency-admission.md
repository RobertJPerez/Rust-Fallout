# Cooperative dependency admission (V3-VM-32)

AdmissionJob retains traversal of caller-selected source roots across bounded
advance calls. It borrows existing immutable PreparedSources and Attachments;
it never parses definitions again, accesses live script banks, executes a native
or creates a continuation save. The existing check and Request::check use the
same engine and keep their five limits, report JSON and canonical ordering.

AdmissionJob::new takes borrowed sources, attachments and exact roots, existing
Limits and separate JobLimits. StepBudget admits definition expansions and
operand visits independently. advance returns historical Progress, progress
observes it, and consuming finish returns a complete Report or the stored
failure/Incomplete. Dropping cancels. Failed or unfinished jobs expose no
Report; completed and failed advances are idempotent.

Roots sort by ScriptKey and duplicate identical roots collapse. Conflicting
versions refuse. The private frontier and scheduled-key set borrow immutable
handles/keys, and an active cached-plan cursor keeps its next operand. Each
definition expands once and each operand visits once, including own writes and
caller prefixes that produce no dependency edge. Shared and repeated source
edges remain in physical operand order. Definitions use canonical breadth-first
order; the first unsupported operation keeps source header priority.

Opening a definition admits its complete instruction and operand-use totals
before operation findings or edges. This preserves existing failure priority and
the exact first excluded SCDA operand even when actual visits span calls. Zero
step allowances can stall without repeating work. A host detects that stall
and drops the job.

JobLimits defaults are frontier 256, cumulative variable-copy reservations
2 MiB, conservative foreign lookup source visits 16777216, and one indivisible
instruction scan 65536. A definition's operation scan, one operand's at-most-
maximum_definitions targets, and final bounded graph cycle analysis are explicit
indivisible units. Definition/edge/operand totals never reset between advances.
There is no hard wall-clock deadline guarantee.

Handle/key/digest/detail extents are charged before output copies. Raw root
string extents are admitted before comparisons and canonical output copies are
charged separately. Existing static quest declaration lookup receives a
reservation for its borrowed source/context/target/declaration strings before
it constructs a diagnostic. Every such lookup conservatively charges the full
source receipt list, including early rejections. Existing record_scripts range
lookup is charged for both temporary owning keys. Formatted cached-source error
text is counted before String allocation. Cycle adjacency/colors/stack borrow
keys; only returned back-edge keys are copied and charged.

The historical one-shot wrapper uses compatibility allowances for these extra
job limits so accepted default calls and public five-limit behavior do not
change. The bounded job and CLI explicitly select finite extra ceilings.
Counters describe admitted logical copies/work, including conservative
temporary reservations, rather than peak heap usage.

The actual entry is source-plans --cooperative-admission. It conflicts with the
existing execution-admission, cooperative-preparation, selected-source and
comparison-bundle requests. Existing routes and their seven-argument inspector
remain unchanged. The new inspector still performs existing bounded synchronous
catalogue/cache preparation before its first admission advance; this command
does not claim an end-to-end responsive boot.

The strict schema-1 request is at most 64 KiB. It requires exact source cohort,
roots, all five existing total limits, all four extra job limits, definition/
operand step limits, maximum_advances, required nullable cancel_after_advances,
maximum_progress_bytes and maximum_report_bytes. Total/step ceilings may only
reduce existing defaults. The advance ceiling is 4096, compact accumulated
progress-row ceiling 2 MiB, and full pretty report ceiling 8 MiB including its
newline. Failure text and progress are admitted before allocation/retention,
and the complete typed borrowed report is counted before JSON Value creation.

Completion publishes the existing complete Report as execution_admission.
Cancellation, exhausted advances, step stall and failure publish only progress
diagnostics with execution_admission null. Progress report publication does not
mean an admission Report exists. Complete structural diagnostics still leave
faithful_execution_admitted false and the CLI exits unsuccessfully. Existing
fresh create_new/protected-source emission applies; interrupted output is
incomplete. No live state changes occur.

The independent authored four-node graph expands A,B,C,D once. Physical edges
are A-to-B, A-to-C, repeated A-to-B, B-to-D, C-to-D and D-to-A, preserving six
edges/visits and fourteen instructions. Canonical cycle analysis reports only
D-to-A. The first unsupported operation is A's native at SCDA 10..19, operand
14..19. One-shot, one-node/one-operand and several mixed schedules produce the
same complete expected report; the one/one host uses six advances. Static
foreign sources have own-write, prefix and read bindings, three visits each.
Cached source rejection reports its exact failure without another parse.

Engineering source admission remains separate from Original semantics. This
change proves no activation schedule, numeric coercion, native return, route,
checkpoint or gameplay acceptance.
