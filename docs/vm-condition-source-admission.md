# Exact winning-record condition source admission

`condition_operands::prepare_record` reads the requested whole-record winner from
the existing strict `RecordStore` reader. It rejects a nonwinning location and a
deleted winning source before reading its deferred body. It tightens stored and
decoded byte bounds before allocation. It rejects checksum-recovered bodies even
when a caller configured its store for forensic recovery.

The returned immutable record keeps its canonical source key, winning header,
decoded-body digest, complete source-file digest and digest of ordered complete
source receipts. The receipt digest has the `FNVCTDASOURCES1` domain prefix. It is
source provenance, not the runtime catalogue identity or authority to mutate state.

Each site preserves its physical order, raw 20/24/28-byte CTDA body and digest,
decoded field-header offset, preceding field kind and offset, existing typed
binding and source findings. Absent words stay absent. Unknown descriptors,
selectors and target dependencies retain the existing unresolved states.
Descriptor signatures are caller-supplied metadata; source receipts do not certify
that metadata. The CLI keeps its fingerprinted executable catalogue validation.
`legacy_row` projects precisely the existing condition inspector schema, including
its binding digest input. No second CTDA parser is introduced.

Admission limits default to 64 MiB stored/decoded bytes, 1,048,576 visited fields,
1,000,000 CTDA sites and 128 MiB retained compact legacy JSON plus raw CTDA bytes.
Sites are counted before retention. Serialized accounting includes escaped strings
and UTF-8 bytes; it does not claim to measure Rust heap usage, pretty-print output
or arbitrary downstream consumer output. Failure returns no prepared record.

The `condition-dependencies` consumer additionally limits candidate locations to
262,144 before collecting them, all decoded candidate bodies to 512 MiB before
reading the next body, and all retained compact record rows to 128 MiB before
appending them. Its default schema, source findings, ordering and output bytes
remain unchanged for admitted inputs. A report with findings still returns 1.
Actor's PACK source audit is the other agreed consumer of this shared helper.

Physical CTDA adjacency and authored OR bits do not establish an owning evaluation
group. The helper neither evaluates conditions nor chooses default subjects, reads
live values, performs numeric coercion, dispatches native behavior or acknowledges
events. Runtime continues to own canonical state, events and saves.

VM-03 original numeric/branch/native behavior remains unsupported: the available
repository oracles read source structure and executable descriptors without
calling original handlers. The active shared heavy proof reservation is not a live
original script measurement backend. No invented numeric rule clears that gate.
