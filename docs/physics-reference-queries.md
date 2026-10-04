# Canonical reference collision selections

`fallout reference-collision` strict-loads an existing native repository with the
current source catalogue, then joins a selected resident model to existing
canonical references. It does not register references, change World, create or
repair saves, or infer the query frame from either source DATA or saved pose.
The published runtime has canonical reference pose state; that pose is preserved.

```text
fallout --output report.json reference-collision --install INSTALL \
  --load-order order.json --save-root NATIVE_REPOSITORY \
  --editor-id CELL_EDID --request request.json
```

Optional `--index-cache` and `--source-cache` use the existing bounded producers.
Report and cache destinations must resolve outside both installation and input
repository; checks precede repository opening. Output uses create-new semantics.

The request supplies `model_index`, exact captured `source_sha256`, `placements`,
explicit `units`, `io_deadline_ms` (1..60000), and `ray` and/or `overlap`.
Each placement supplies a nonzero integer `reference`, exact `authored` FormKey,
nonempty `body_blocks`, and complete `attachment_rows` (three rows of four
binary64 values). `verify_unload: true` additionally checks receipt revocation
and release of all selected geometry/model/plan pins. Unknown fields refuse.
ReferenceId stays an integer, including values above the binary64 exact range.

`physics::reference::ReferenceCollision::admit` takes an immutable World, original
CellResidency/ticket, protected mutable RecordStore, model index, explicit
ReferencePlacement inputs, engineering units and lowerable reference Limits.
Admission checks both canonical origin maps, protected source receipts, the
private catalogue winning-definition digest, and the sealed resident plan.
It requires one exact CELL member → placed NAME → winning base MODL → captured
model association, re-reading the winning placed/base records with the existing
bounded decoders and checking headers and decoded hashes. Deleted, unknown,
wrong-kind, ambiguous, duplicate and missing associations refuse.

The cache retains the upstream model lease and borrows the protected RecordStore.
Source handles therefore outlive usable results. Opaque `ReferenceHits` provide
checked borrowed `hits` and `scope`; neither public diagnostic Scope nor JSON
can authorize a new selection. Queries and access check World epoch, campaign,
catalogue fingerprint, revision, both maps, original cell owner and exact ticket.
Equal-state cold restoration creates a different World epoch. Release, drop,
replacement failure, canonical invalidation and cell unload revoke old receipts.
Canonical mismatch also drops the selected geometry. Budget refusal emits no
partial result and allows retry of an otherwise current selection.

Default reference admission ceilings are 1024 placement bodies, 256 plugin
sources/4 GiB, four million visits, 4 MiB per record, 64 MiB aggregate decoded
bytes (including upstream plan decode and selected captured model), 262144 field
sites and 64 MiB logical metadata. Existing scene caps also apply. Typed decoder
scratch is checked before decoding. Scope includes charged usage; these logical
accounting estimates are not a measurement of process peak memory.

The CLI additionally preflights plugin bytes, splits a total four million indexed
records and 4 GiB index decode allowance equally across plugins, and admits at
most eight archive sources/16 GiB. Uneven cohorts may refuse under those explicit
engineering shares. Catalogue, plan and reference phases each have a fixed
64 MiB record/model decode ceiling (192 MiB aggregate ceiling, with shared plan
work conservatively charged again). Catalogue retention is capped at 16 MiB;
existing plan/probe/residency ceilings apply. Those phase allowances do not grow
with placement count. At most two queries share total defaults of 100000
primitive tests, one million geometry tests and 10000 hits; reference checks
share a fixed 4096 allowance. A counting serializer checks the entire pretty
report plus newline against 64 MiB before output allocation/publication.

Authored tests use literal plugin/BSA/NIF sphere geometry, two placed records
sharing one STAT, and distinct IDs `0xf000000000000010` and
`0xf000000000000011`. Source positions (33,44), saved positions (100,200) and
explicit query translations (0,10) intentionally differ. A ray from -3 along X
has analytic entries 2 and 12; a radius-zero overlap at the first center selects
one reference. Negative fixtures and exact/one-under budgets check atomic
refusal and unchanged snapshots. An explicitly ignored fixture exporter enables
the private frozen-CLI proof; it is not a production auto-registration consumer.

These are selected core geometry engineering queries. Raw filters/materials and
source attribution remain intact; source shell-margin exclusions and numeric
refusal scopes remain those of the existing static scene. Cell collision stays
Unsupported, simulation readiness and `faithful_ready` stay false. No actor
movement, canonical pose conversion, character stepping, Havok simulation or
original gameplay acceptance is established.
