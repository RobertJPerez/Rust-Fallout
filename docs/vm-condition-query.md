# Source-bound condition host observations

`execution::condition::Request` borrows one immutable winning CTDA record and
selects its exact decoded field offset. Preparation checks every ordered source
receipt against the canonical world's catalogue. Observations retain campaign
and complete cohort identity and resolve current state on each call, including
after a same-campaign restore. They contain no saved continuation or state bank.

Engineering observation routes only condition function 47 with the bound single
type-50 parameter and a defined content FormID through the existing host query.
The source must select `Subject`; the caller must supply an explicit live subject.
Absent, target, reference, combat-target and linked-reference selection remain
unsupported. Missing/deleted/null/runtime arguments, unverified descriptors,
source findings and nonzero unused parameter words also remain unsupported.
Form-list expansion uses the shared query's explicit refusal. Contribution
capacity failures abort without partial results.

Successful observations expose the exact integer host `CountTrace`, including
current revision, clocks and item contributions. They leave the original numeric
return and condition truth absent. Faithful intent remains unsupported. No
comparison, AND/OR evaluation, default subject, event acknowledgment or mutation
is performed; an authored OR bit stays visible in the source site.

The existing condition inspector accepts an optional input:

```powershell
fallout.exe condition-dependencies --install INSTALL --load-order order.json `
  --engineering-query-input request.json --output report.json
```

```json
{
  "record": {"profile": "nv-original", "origin_plugin": "base.esm", "local_id": 1024},
  "field_decoded_offset": 0,
  "explicit_subject": 1,
  "snapshot": "snapshot.json"
}
```

The input is limited to 16 KiB and its canonical runtime snapshot to 64 MiB.
Relative snapshot paths resolve beside the request. Runtime's existing bounded
snapshot decoder and `World::restore` validate the snapshot against the loaded
catalogue; the inspector creates no state or initialization defaults. It emits
both Faithful and Engineering observations for the selected site, with a limit
of 65,536 contributions. Query reports are admitted before JSON retention and
the combined report is bounded to 128 MiB. A missing/deleted selected record,
missing site or invalid snapshot aborts without publishing a result.

The opt-in report uses schema 4. Existing schemas 1–3 keep their source projection
when the option is absent. Native and independent raw readers establish source
association; authored exact counts and cold snapshots establish the engineering
host behavior. They do not measure original condition handlers or accept retail
gameplay parity.
