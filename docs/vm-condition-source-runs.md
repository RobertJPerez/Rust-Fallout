# Physical condition source runs

`condition_operands::prepare_source_runs` borrows an immutable
`PreparedOwnerRecord` and its complete source identity. It describes source
adjacency for mapped QUST/INFO CTDAs. It holds no runtime state or live values.

A run extends to the next CTDA only when the current raw OR bit is set, both
sites have the same nonnull section owner, and their field headers and bodies
are physically adjacent. An intervening field or `XXXX` framing breaks the
run. Site indices refer to the complete physical CTDA sequence, including
unowned sites. Orphans and unmapped record kinds acquire no runs.

Each run records its owner, first site, exclusive end site, unchanged tail OR
bit and end reason. Reasons use this priority: `raw_or_clear`, `record_end`,
`unowned_next_site`, `owner_change`, then `physical_field_gap`. A true OR tail
at a record, owner or physical boundary stays true. Repeated stage/objective
keys remain separate sections. These spans do not establish Boolean grouping,
evaluation order, short circuiting or a default subject. `evaluation_ready`,
`group_evaluation_verified` and `default_subjects_applied` remain false.

`RunLimits` defaults to one million runs and 128 MiB of compact serialized
metadata, including the borrowed identity projection. Each run is admitted
before retention; final serialization must match the counted bytes. These
limits bound retained source/report work rather than total process heap.
Existing strict record and owner admission remains in force.

The existing condition inspector consumes the adapter:

```powershell
fallout condition-dependencies --install <installation> --load-order <order.json> --include-source-runs --output <new-report.json>
```

The flag implies owner inspection and emits schema 3 with per-record
`source_runs` and aggregate `source_run_counts`. Supplying both source flags
has the same result. The default schema 1 and owner-only schema 2 retain their
previous bytes. The whole record, including both additional metadata trees,
must fit the inspector's aggregate retention budget before either tree is
materialized. Existing diagnostic findings and exit behavior remain in force.

Focused tests cover literal spans, true tails, owner changes, orphans, unmapped
records, intervening fields, extended framing, exact count/byte limits and
complete cohort identity. Private comparisons use an independent raw field
reader and native narrative reader for authored inputs and two selected
original physical records. Selected records retain exact stored/body hashes
and original offsets in a derived private container; they do not represent the
complete installed winning order. Existing report bytes and cold/warm cache
projections are compared separately, and altered span, identity, owner, raw-OR
and evaluation fields must reject. This evidence establishes physical source
structure; original runtime command and condition behavior remains unmeasured.
