# Fallout 4 small-master record identity observations

The current physical census contains nine plugins whose TES4 header carries the
FO4 small-master flag `0x00000200`. This audit reads only those source files and
retains each major-record header's exact raw FormID, record offset, source hash,
and raw high byte. It does not inspect or resolve nested reference fields.

The pinned Mutagen source at revision
`4f533562ee0c70347d47c1979d5464d42b06ee6b` identifies the Fallout 4 small flag
and documents the `0xFE` marker layout as a 12-bit master selector plus 12-bit
local candidate. The same source shows a separate full-width representation.
This tool reports those bit patterns as observations only. It never assigns a
`FormKey`, assumes an out-of-list selector means the current plugin, maps an ESL
runtime slot, or chooses an override/load-order winner. The four previously
preserved VMAD master-slot findings remain unresolved.

Reproduce against the frozen content proof and a read-only retail installation:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bin audit-esl-ids
.\local\target\debug\audit-esl-ids.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local/proof-fo4-002 local/esl-id-audit-new
```

The command checks each plugin's size and fingerprint against the completed proof
before scanning, then hashes it again after the scan. Output is confined to a new
ignored `local/` directory and `complete.json` is written last. The detailed
per-record rows are private evidence at `local/esl-id-audit-001`; see
`reports/formid-checkpoint.json` for aggregate counts and evidence hashes.

In the observed proof, all nine small-flag plugins list one master. The scan
counted 5,682 record headers: nine zero values and 5,673 full-width patterns.
None of the record-header IDs used the `0xFE` or `0xFD` marker pattern. Raw high
byte `0x01` occurred 5,514 times, outside the one-entry listed-master table; the
audit leaves those values unassigned. Raw byte `0x00` occurred 168 times,
including the nine zero-valued headers. All source hashes matched before and
after.

This result separates the small-master header flag from the observed record
header bit pattern. It does not establish how Fallout 4 assigns these IDs at
runtime or how other plugins' reference fields address them. Active profile,
ESL master mapping and override behavior remain open.

The workshop audit now emits an additional raw slot ledger for pinned-schema
FormLinks in COBJ, CMPO, and MISC records. It includes exact decoded offsets,
record/plugin provenance, the listed-master count, raw values, and bit-pattern
candidates. CTDA `Reference` links are included; parameter slots are included
only for functions whose pinned map explicitly classifies them as FormLinks.
Unknown/defaulted slots stay in the raw condition payload and are not promoted
to links. CMPO crafting sounds and the direct MISC transform, sound, keyword,
message, and scrap-component links are also represented. GLOB is represented by
its record header only.

On the frozen 16-plugin proof, the slot ledger contains 50,303 rows: 10,239
record headers and 40,064 nested slots from 8,296 COBJ/CMPO/MISC records. The
independent pinned reader matched all 37,699 nonzero nested local-ID candidates;
the other 2,365 raw zero values remain preserved. This is a structural check of
the audited workshop fields, not a general plugin reference scan or a runtime
identity resolver. It does not cover VMAD object values, placed references, or
other record families. See `reports/workshop-checkpoint.json`; detailed rows are
kept under ignored `local/workshop-recipe-018/formid-slots.jsonl`.
