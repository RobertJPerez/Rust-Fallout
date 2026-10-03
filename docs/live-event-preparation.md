# Preparing a source-bound pending event

`World::prepare_event` joins one pending journal sequence to its live script
instance, retained context and exact immutable winning source version. It returns
a read-only `PreparedEvent` containing the pending entry, instance, complete source
plan, selected authored event and that event's instruction window.

Selection uses the authored BEGIN byte offset and event ID together. A script can
contain several blocks with the same ID; choosing the first matching ID would
select the wrong source region. The sequence lookup uses binary search over the
strictly ordered deque, including when its allocation has wrapped.

Preparation borrows the world. The instance, context, source and queue cannot be
mutated through the returned frame. No pending entry is acknowledged, local value
assigned, branch chosen or native command called. Success establishes structural
source readiness; it grants no permission to execute bytecode.

```rust
let frame = world.prepare_event(sequence, &model, &signatures, limits)?;
let authored_window = frame.instructions();
// Both the BEGIN and END headers are included in source order.
```

The complete definition passes the existing source-plan checks first: exact
source identity, source metadata, delimiter relations, postfix structure and the
unit's own operand tables. Unresolved findings remain failures. The selected event
also has a separate instruction limit; an over-budget frame fails rather than
returning a truncated window. Missing or acknowledged sequences fail explicitly.
Object-event masks retain their observations, but mapping them to source block
IDs is unverified and therefore rejected by this API.

Eight runtime tests cover exact instance/context/source identity and unchanged
locals, repeated event IDs at distinct offsets, missing/acknowledged sequences,
unverified object masks, source-table/control failures, exact budgets, owned-world
restoration and stale handles, and wrapped queue lookup. The snapshot/native-save
schemas and acknowledgment policy do not change.

## Installed-data inspection

The `event-frames` command restores the existing explicit engineering state onto
an owned immutable catalogue and attempts preparation of every pending entry. Its
comparison bundle contains the exact successful event windows in `FROBS001`
framing. Raw windows and source-bearing reports stay in ignored local directories.

```powershell
.\target\release\fallout.exe event-frames --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --load-order profiles\nv-inspection-order.json --comparison-bundle local\YOUR-NEW-FRAMES.bin --output local\YOUR-NEW-REPORT.json
.\target\release\fallout-evidence.exe --checkpoint 43 --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' --run-directory local\YOUR-NEW-PROOF --no-publish
```

The development comparison checks 2,160 pending entries. It prepares 2,111 frames
with 50,415 instruction headers and retains 49 existing control-structure findings.
Consequently, the diagnostic inspector writes its report and returns exit code 1.
Cold and warm reports agree exactly apart from cache reuse metadata. The successful
full plans contain 85,708 operand uses; all attempted source bodies account for
1,168,191 bytes, including the attempts that retain findings.

The inspector bounds pending rows, exported bytes, full-plan instructions,
expression tokens/nodes and operand uses. It charges source bytes before each
attempt, including failures, so repeated failed preparation cannot escape that
work budget. Per-definition limits remain active as well. Successful-result
counters do not claim the exact internal work performed before a failed decode.

## Evidence and limits

The checkpoint runner reruns the full source-plan/shared-runtime regression and
joins event rows to its separately persisted engineering journal. Every pending
sequence, context, instance and source definition must match. The selected event
must belong to its complete source plan, and the full operand-binding digest must
agree with that definition's owning tables. Every exported window is checked
against its exact winning compiled region and a separate native header reader.
Source-byte and operand-use totals are derived again from the fresh source reports.

The unchanged canonical snapshot has SHA-256
`0679d4a569e24501f2fd2df9a37035b8e83288a2bb2d44385b02f281450117c6`.
These contexts and local values are deterministic engineering inputs, not captured
retail initialization or event dispatch. The reader comparisons prove source facts;
the preparation/state checks prove engine guarantees. Full proof results are
published separately in the checkpoint receipt.

Live operand readiness, original event arguments/lifecycle, numeric/coercion rules,
branch/native effects and persisted execution continuations remain open. Actor
initialization, scheduling and gameplay scenarios remain unaccepted. The next
runtime work connects exact source operand associations to explicit live values
and capabilities without substituting authored attachments for missing instances.
