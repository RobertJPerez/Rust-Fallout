# Script candidate read admission

`loaded_scripts::Catalogue::load` bounds read work separately from retained
script bodies. `Limits::max_candidate_read_bytes` defaults to 512 MiB and
`max_candidate_record_bytes` to 64 MiB. Every nondeleted winning candidate
must fit both the per-record bound and the aggregate allowance remaining.

The catalogue uses the existing `RecordStore::read_bounded` before decoding
script fields or retaining shared bodies. The reader tightens stored and
decoded body limits together and keeps stricter store limits in force. Stored
size rejects before allocating its body; a compressed body's declared decoded
size rejects before allocating the decompression buffer or invoking zlib.
Existing source/header and checksum checks remain in force.

Every successfully read candidate charges `max(stored_size, decoded_size)`
once, including records with no script units. Compressed overhead therefore
counts when the stored body is larger. Deleted, unrelated and nonwinning
records do not consume this allowance. A rejected construction returns no
partial catalogue; retry starts a fresh admission counter. Existing observers
still receive only successfully admitted records containing scripts, so
observers must continue to treat catalogue construction as fallible.

The existing retained-byte limit keeps its meaning: decoded bodies held by
loaded script definitions. Serialized catalogue counts keep their previous
shape, including decoded bytes scanned and retained. These bounds describe
conservative source admission rather than total process heap, summed stored
and decoded work or decompression CPU time. A reader budget error preserves
its source name, record offset and existing format diagnostic.

The current `loaded-scripts` inspector and prepared-source/event consumers use
the limits through their existing catalogue load. No new command, runtime
store, event policy or save format is introduced. Inputs inside the admission
bounds retain their previous reports and script handles.

Private characterization records the prior empty-candidate read work and the
bounded reader's predecode error. Focused tests cover exact and one-less read
allowances, stored compression overhead, aggregate remaining capacity after
earlier success, empty candidates, retry, source-store bounds, skipped records,
retained limits, malformed metadata and existing checksum recovery rejection.
An independent raw reader measures stored/decoded extents; the frozen native
catalogue reader compares admitted script identities and metadata. These
checks establish source admission and leave original runtime behavior
unmeasured.
