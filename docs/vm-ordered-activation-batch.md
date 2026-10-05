# Ordered activation event batch

`fallout_runtime::execution::pending_batch::Job::new_ordered` consumes an
explicit prefix of the existing pending-event journal for a behavior tick. Each
request pairs the saved sequence with the current instance's source-qualified
`Owner`. Construction validates the full prefix before any private event is
committed, so a stale owner, reordered sequence, or missing journal row cannot
silently select a different script.

The consumer reuses the existing source-frame admission and copy adapters:

- `Fragment` owners use the existing single numeric own-local copy adapter.
- `Quest` and `Placed` owners use the existing bounded multi-copy adapter.
- Only supported source-authored own-local numeric copies commit. Literal or
  computed operands, foreign reads, unsupported statements, and faithful-mode
  requests refuse explicitly.

Each event runs in sequence against the preceding private result. The batch
keeps one aggregate event, source-instruction, statement-byte, and trace
projection budget. `advance` counts complete events and does not repeat a
committed private event. If any later event is unsupported or exceeds a limit,
the private world and every earlier event receipt are discarded and no
candidate snapshot is returned.

On complete success, `finish` returns a candidate snapshot and `ordered_events`
receipts in journal order. Full traces are in `committed` for Fragment copies
and `committed_multi` for Quest/Placed copies. The older fragment-only
`Request { sequence, activation }` and saved-batch CLI output stay unchanged.

The application host should retain its canonical world while this private job
runs, then install the returned snapshot only after full success. It supplies
the exact queued prefix and reads each owner from that event's current instance;
the runtime does not guess dispatch order, activate owners, or map object-event
masks to source blocks. This is an engineering consumer of the implemented
copy subset and makes no claim about original-game execution or gameplay
acceptance. Persisted suspended jobs remain a separate follow-on.
