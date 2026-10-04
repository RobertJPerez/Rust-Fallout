# Engineering source copy across native saves

The runtime save tests consume scripts' tested `VM-02` local-copy adapter and
runtime's existing opaque event transaction. The authored source contains one
own-local assignment between Begin and End. An explicit host input initializes
the numeric source with NaN payload bits; copying those bits is an engineering
operation. Original numeric conversion and event lifecycle remain unsupported.

`save_local_copy` checks whole snapshots across these boundaries:

| Boundary | Expected persistent state |
| --- | --- |
| Before source copy | Source initialized, destination unset, both queued events present |
| After source copy | Exactly one destination assignment, first head acknowledged, revision incremented once, second event retained |
| Cold previous recovery | Exact before state, explicit current failure, no slot repair or file changes |
| Fresh copy after repair | New staging authority over restored state, byte-identical current/previous containers |
| Cold continuation | Second exact head acknowledged, one revision increment, empty pending queue |

The bounded worker accepts captures with a 4 KiB declared snapshot limit and
uses either one or two such reservations. Captures survive later host mutation
and destruction of the original world and source catalogue. New processes reload
the source cohort, open the existing repository and compare the entire restored
snapshot with the expected boundary. Source/definition identities, campaign,
allocation counters, local bits, contexts and pending tail therefore remain part
of the comparison. The cold helpers also check generation, container/snapshot
hashes and unchanged slot files.

Failure checks retain the original capture on byte admission rejection and leave
the existing slot files unchanged. A busy writer reports failure without undoing
the already committed in-memory transaction. Retrying requires an explicit
capture. Dropping a result ticket still permits the accepted save to complete
before shutdown. Restarting the worker rejects an older capture, retains the
latest save, and can persist a fresh stage over the restored remaining head.
Dropping an uncommitted stage and faithful unsupported intent leave state and
slots unchanged.

Run the focused regression with the project's pinned Cargo wrapper:

```powershell
py -3 G:\Rust-Fallout\tools\team-v2-focused.py --lane runtime -- powershell.exe -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 test --locked --jobs 2 -p fallout-runtime --test save_local_copy --test save_worker --test saves --test execution_local_copy --test execution_native
```

Normal tests use temporary fixtures. Setting `FALLOUT_SAVE_COPY_EVIDENCE` to a
new private evidence directory retains authored inputs, expected snapshots,
containers, traces and cold process receipts in fresh `boundaries` and `failures`
subdirectories. Existing subdirectories cause failure instead of overwriting
evidence. The ignored helper is launched by the parent tests; running the parent
tests exercises it automatically.

These checks establish engineering save/restore behavior for the actual adapter.
They introduce no save fields, persistent continuations or retail execution claim.
