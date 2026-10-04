# Source-bound native capability and dispatch

`World::prepare_native_calls_with_sources` selects one exact pending event from
the existing immutable source plans. It inventories physical instruction and
expression native occurrences in source order, including duplicates and calls
inside branches. It does not choose a branch or produce an executed trace.

The borrowed `NativeCalls` holds the current world and prepared event. A caller
can inspect its calls or request an observation by occurrence index. Calls retain
their command ID, owning instruction index and extent, native/token extent,
argument offset and bytes, and source caller index.
Each observation retains campaign, full source cohort, current revision, pending
event, definition version, owner and every event-context role. An owned JSON
observation is evidence, never authority for a later read or write.

The coordinator approved `VM-01-native-v1` in the central team-v2 board. Runtime
owns state, events and saves. This adapter only reads existing canonical APIs;
it has no state bank, journal, save field, staged mutation or acknowledgment.

## Capability and outcomes

Decoded source, engineering host reads and retail execution are independent
facts. An arbitrary command ID does not establish decoded source. A prepared
call does. Only native command `0x102F` has an engineering host-read route, through
the existing `query::Request`. CTDA function 47 stays a separate identifier.
No native command has an accepted retail execution handler in this slice.

`Intent::Faithful` returns typed `UnverifiedRetailSemantics` for GetItemCount.
Other commands return `MissingImplementation`. There is no successful no-op,
invented return value or conversion from the host count into an original number.

`Intent::EngineeringObservation` admits a small source argument slice: one
reference-table operand with the already evidenced default signature (required
parameter type 50). The existing argument decoder reads it; canonical reference
resolution must produce a content key. A prefixed caller must resolve to a live
identity. An unprefixed call requires `Inputs::supplied_subject`; owner, pending
caller, containing reference, target and explicit player never supply a default.
The explicit player input is used only by canonical source-reference resolution.
The shared source planner rejects reference prefixes on statements before any
native frame can be admitted; no enclosing caller inheritance is guessed.

The common host query returns its exact checked quantity and bounded contribution
trace. `original_numeric_return` remains absent and `original_behavior_verified`
remains false. Form-list expansion, form-variable admission, null/live arguments,
unresolved callers, missing banks and unverified argument tails remain explicit
unsupported outcomes. Exhausted query budgets are errors, not semantic failures.

Native occurrence and aggregate argument-byte limits apply before retaining any
call; event instruction limits include delimiters. A failed admission yields no
partial `NativeCalls`. Source/cohort rejection precedes dispatch. Observations
must be prepared again to see a changed state; restoration uses persistent
identities and newly checked handles.

## Reachable inspector

```powershell
& .\target\debug\fallout.exe event-operands `
  --install 'G:\SteamLibrary\steamapps\common\Fallout New Vegas' `
  --load-order 'G:\Rust-Fallout\profiles\nv-inspection-order.json' `
  --native-capabilities --output .\local\native-capabilities.json
```

The additive flag prepares sources and adds schema 3 observations in faithful
intent. Unsupported native semantics cause the command to return 1 after writing
the report. Default schema 1 and prepared-source schema 2 remain unchanged. Native
calls and argument-byte work share batch limits of 2,000,000 and 16 MiB; the existing
128 MiB report limit still applies. This inspector never invokes engineering host
reads or executes bytecode. The source-bound engineering route is exercised by
the focused runtime integration cases using explicit known inventories.

## Evidence boundary

The existing primitive-query descriptor evidence binds the fingerprinted retail
executable (`3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d`).
Pinned xNVSE revision `0ccd23ad885ddae533c1790a3fc56cd073e38de3`, complete
`nvse/nvse/CommandTable.h`, distinguishes command context and parameter types;
it supplies no portable original GetItemCount algorithm. The existing independent
argument/binding readers establish source layouts, not execution semantics.
No upstream implementation is copied or linked by this adapter.

Focused cases use authored source-less compiled fixtures and independently
specified source offsets and inventory quantities. They cover wide exact counts,
caller/context distinctions, event isolation, unsupported commands/form lists,
admission budgets, source rejection, current-state reads and restored identities.
These are engineering checks. Original argument admission, numeric coercion,
form-list behavior, expression effects, branch rules and scheduling need isolated
retail measurements and a separate coordinator reservation. No gameplay parity
or accepted route is claimed.
