# Explicit conversation source batches

`DialogueSources::prepare_batch(store, requests, signatures, limits)` consumes
1–8 explicitly chosen sealed topic/INFO/speaker requests. The private immutable
`ConversationBatch` retains the existing `ConversationSources` for each request.
Borrowed conversations expose exact original subtitle spans and existing
source-bound fragment requests. Caller order remains caller order; it does not
establish dialogue selection or eligibility. Exact repeated tuples refuse.
The same INFO with different explicit speaker declarations remains distinct.

The constructor checks complete ordered source names, sizes and digests, sealed
membership, winning physical topic parents and optional live actor header
identities. It preflights every topic and INFO with the existing strict bounded
reader and subrecord visitor. Aggregate field, condition and script-unit counts
and copied source metadata are admitted before any typed closure is prepared.
Section slots conservatively reserve a possible section per source field,
including the INFO owner's duplicate section table, rather than adding another
ownership parser.

The existing single preparer receives the exact reserved topic/INFO/condition
read work and remaining cumulative retained allowance. It continues to use the
existing narrative, physical condition-owner and fragment helpers. Each result's
raw payloads plus compact metadata are charged exactly before batch retention.
A last source, condition, role or retention failure drops the whole result.
Repeated or unknown fragment roles refuse the new batch; the existing single
request command and producer retain their current behavior.

Ten lowerable ceilings bound requests, per-record max(stored, decoded) size,
aggregate read work, raw topic/INFO payload copies, source fields, section slots,
conditions, fragments, copied source metadata and retained closures. Read work
includes the raw preflight plus the existing topic/INFO reads and condition INFO
reread. Repeated topics are conservatively charged per request. Source digests
and the separately bounded membership index/catalogue retain their existing
scope. These are logical scoped admission limits, not a process-wide peak-memory
or allocator-overhead promise.

One batch seal binds the complete source cohort and ordered request/closure
identities. Every closure identity includes unchanged source field spans, raw
words/hashes, parent identity, explicit speaker and condition/fragment metadata.
Limits and usage do not change the content identity. Revalidating a batch checks
the entire ordered source cohort, including deferred unrelated plugin bytes.
Metadata JSON cannot construct an admitted batch.

Independent authored sources contain a literal 39-byte topic and two unequal
185-byte INFOs with repeated/non-UTF8 NAM1 payloads, unusual CTDA words, original
response numbers and begin/end script units. Fixtures check physical offsets,
decoded spans, real retained subtitle and loaded fragment consumers, canonical
two-master overrides, compressed/XXXX fields, final-request refusal and every
exact/one-under bound. No selection, condition truth, default speaker, UI/voice
playback, fragment execution/timing or retail parity is admitted. The actual
owned headless consumer and frozen installed pair observation follow the
producer handoff.

The owned `conversation-batch-sources` command accepts `--install`, `--load-order`,
optional `--index-cache`, `--requests PATH`, and optional `--bind-result-fragments`.
The protected input is at most 64 KiB and uses this closed schema:

```json
{"schema_version":1,"requests":[
  {"topic":"Base.esm:100","info":"Base.esm:300","speaker":"Base.esm:400"},
  {"topic":"Base.esm:100","info":"Base.esm:301"}
]}
```

Only canonical request strings and an optional explicit speaker are accepted;
unknown/duplicate fields, invalid versions/types, empty/oversized lists and
forged source metadata refuse before source access. Source preparation failures
emit a null batch with empty subtitle/fragment lists and return nonzero. The
consumer walks each retained response field once, preserving request/response/
occurrence/field ordinals, byte lengths and SHA-256 without emitting story text.
Requested fragment binding reuses the existing loaded catalogue and exact
fragment resolver. All output is published together after every binding and
final source validation succeed. Optional binding remains a source definition
observation; execution, timing, selection, condition truth and runtime readiness
remain false. The original single-request command retains its existing behavior.


## Canonical source membership pages

`DialogueSources::page(topic, speaker, cursor, PageLimits)` returns private sealed
requests from the existing membership index. Canonical key sorting is an
engineering lookup order. Deleted and moved winners stay excluded from their old
topic; overrides retain their canonical origin key. No INFO bodies are read.

`PageCursor` is opaque, neither cloned nor serialized. It binds the exact index
instance, complete source cohort, topic, explicit speaker, next ordinal and last
canonical key. It cannot continue another index even with identical sources.
Its small index token does not keep the membership graph alive. Metadata JSON
contains no continuation authority. Pages may be retried from the same borrowed
cursor; continuation never mutates the index.

Limits admit 1–1024 returned members, 1–32768 reserved member visits and
1–1048576 logical copied bytes. All copies are charged before allocation.
Visits conservatively reserve each bounded membership binary search, both page
passes and two cursor progress witnesses. Copied bytes count actual cloned
strings plus five fixed logical bytes per canonical key (profile and local ID),
including every request, the next cursor and the page cohort. These bounds
exclude allocator overhead and the separately admitted immutable index.
An absent topic returns explicit empty structural membership, charging its
cohort string; it establishes no playable empty menu.

The caller selects a returned `Request` and passes it to the existing single
`prepare`, which still validates the live full source cohort and winning physical
parent. Paging alone establishes no eligibility, condition truth, original menu
order, inferred actor, PNAM behavior, voice path or runtime readiness.
