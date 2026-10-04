# Script catalogue integrity admission

`loaded_scripts::Catalogue::load` rejects a winning, nondeleted script-candidate
record with `integrity_issue` before creating a shared body, decoding script units
or calling its record observer. The error identifies the source plugin and record
file offset. A candidate without script units is checked before it can be skipped.

The forensic `RecordStore::open_nv` mode can recover exact decoded bytes from a
zlib stream with a damaged Adler-32 checksum. The recovered record carries the
mismatch for diagnostic inspection. Before this guard, valid-looking SCHR/SCDA
fields in such a record could enter the immutable script catalogue without that
mismatch. That catalogue supplies `programs::PreparedSources` and canonical
runtime worlds. Source version hashes identify bytes; they do not repair source
integrity.

The guard changes no public API, source identity recipe, state bank, event or save
format. Strict loading and valid compressed sources retain their existing
projections. Winner selection and deletion retain their existing rules: a
nonwinning recovered body cannot replace the winning definition, and a deleted
winner cannot resurrect a prior script. This does not accept forensic source
inspection as original gameplay evidence.

Focused cases retain reachable recovered standalone, quest-log and dialogue
fragment records, plus a recovered candidate without units. They verify rejection
before its observer callback, exact decoded bytes and explicit checksum evidence.
Positive cases compare strict and forensic catalogues for valid compressed bodies
and preserve winning/deleted source selection. The tests optionally retain their
authored inputs and observations under an existing private directory named by
`FALLOUT_SCRIPT_INTEGRITY_EVIDENCE_DIR`; each case creates a new subdirectory.

Private comparison checks use an independent strict zlib decoder to confirm each
damaged checksum and recovered byte digest. The unchanged native script catalogue
reader independently compares valid script identities, metadata, owner markers
and source receipts against an independently extracted authored bundle. Its
compressed-record comparison checks the declared decoded extent; the separate
strict zlib check supplies compressed integrity evidence. Existing runtime source,
event preparation, operand and engineering execution tests check compatibility.
No original native handler, numeric rule or condition truth is established here.
