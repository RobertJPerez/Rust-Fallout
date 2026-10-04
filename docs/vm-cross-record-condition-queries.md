# Explicit condition queries across exact source records

`condition::CrossRequests::prepare(world, records, selections, limits)` borrows
privately admitted `PreparedRecord` values. Each `RecordSelection` names a record
index, exact physical CTDA field offset and explicit nullable subject identity.
Selections retain their order, duplicates and nonmonotonic sites. Record inputs
must have distinct canonical keys; diagnostic JSON cannot construct source
authority.

Preparation streams the existing ordered CTDA source receipt domain once. Every
record must match that complete current cohort and its own source name/digest.
Private source preparation already verifies the winning header, decoded source
hash, exact field bytes and immutable binding. A shared binary-search helper
preserves legacy single-record lookup counts. No second parser or index exists.

Limits charge distinct records, decoded source bytes, fields, sites, retained
source bytes, source receipt bytes and comparisons, requests, lookup comparisons
and cloned runtime-cohort strings. A failed or late missing site exposes no
partial batch. `CrossRequests::observe(world, content, intent, limits)` shares one
contribution allowance and charges each serialized observation before retaining
it. Its compact array budget includes brackets and separators. Current campaign,
source cohort and content are checked again; a same-campaign cold restore and
changed inventory are read fresh.

Each observation reuses the existing `Request::observe` adapter. Subjects remain
separate per selection. Faithful intent and unsupported source/query shapes keep
typed refusals. Counts are engineering inventory observations: condition truth,
comparison/grouping, package eligibility and original behavior remain unavailable.
No state or pending event is changed or acknowledged.

## Saved CLI consumer

```powershell
& .\target\debug\fallout.exe condition-dependencies `
  --install .\local\authored-source-copy --load-order .\local\order.json `
  --engineering-query-records .\local\record-queries.json `
  --output .\local\record-query-report.json
```

The strict schema 1 request is at most 16 KiB; every field is required:

```json
{
  "schema_version": 1,
  "intent": "engineering_observation",
  "selections": [
    {
      "record": {"profile": "nv-original", "origin_plugin": "falloutnv.esm", "local_id": 1280},
      "field_decoded_offset": 0,
      "explicit_subject": 1
    }
  ],
  "snapshot": "input.snapshot.json",
  "maximum_records": 64,
  "maximum_source_bytes": 8388608,
  "maximum_record_fields": 262144,
  "maximum_record_sites": 65536,
  "maximum_retained_source_bytes": 16777216,
  "maximum_requests": 4096,
  "maximum_source_receipt_bytes": 1048576,
  "maximum_receipt_comparisons": 262144,
  "maximum_site_comparisons": 1048576,
  "maximum_query_variable_bytes": 1048576,
  "maximum_contributions": 65536,
  "maximum_observation_bytes": 8388608,
  "maximum_report_bytes": 8388608
}
```

The snapshot path resolves relative to the request. It must strictly restore as
current schema 4 under the complete supplied source catalogue. The consumer
loads each selected winning condition record once in first-mentioned order,
tightening decoded/field/site/retained limits before source preparation. All
physical sites are admitted before the first query. The entire canonical snapshot
is checked afterward; the input file is unchanged.

Caller bounds may reduce the shown ceilings. The compact observation budget and
complete pretty report plus trailing newline are independent. The latter includes
metadata and source receipts and must fit before publishing anything. A supplied
report is fresh and outside the installation; shared `emit` uses `create_new`.
Semantic refusals yield a bounded complete report and nonzero exit; identity,
source, lookup, strict restore and capacity errors expose no partial report.
Publication I/O failure can leave an incomplete fresh artifact, never a verified
proof. Old default/single-record modes and their report schemas are preserved.
