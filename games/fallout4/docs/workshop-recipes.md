# Fallout 4 workshop recipe structures

The `audit-workshop` binary inventories Fallout 4 `COBJ` records from the
completed physical retail proof. It preserves raw plugin/FormID provenance and
extracts the COBJ fields needed to plan a future workshop loop: component
quantities (`FVPA`), created-object links (`CNAM`), workbench keyword links
(`BNAM`), categories (`FNAM`), menu art/sounds, and created-object counts
(`INTV`). It keeps record versions per recipe instead of selecting the schema
from the executable marketing version.

The pinned Mutagen schema describes each `FVPA` entry as a raw FormID plus a
`u32` quantity. `INTV` entries have a `u16` count and an optional `u16` priority;
the short form is recorded as absent priority rather than numeric zero. COBJ
conditions (`CTDA`) retain exact bytes and are decoded into the fixed 32-byte
structure described by the pinned FO4 schema: packed flag/operator bits,
comparison bits, function index, raw parameter slots, raw run-on/reference values,
and unknown fields. The parser carries all 479 named functions from the pinned
Fallout 4 schema and its 220 explicit three-slot parameter mappings. The other
259 named functions remain defaulted and unresolved; unknown future indices
remain unknown. The two raw CTDA parameter words are classified only from the
first and second schema slots, while third-slot type metadata is retained without
inventing another wire field. The independent comparison checks function names
and all three parameter types/categories for observed conditions. In the current
corpus, 14 explicit mappings cover 3,609 conditions; the other 17 use function
300 (`IsInInterior`) and remain unresolved.
`CIS1`/`CIS2` companions remain exact opaque bytes; their association with a
condition is not guessed. FormID identity and condition evaluation remain
unimplemented. Unknown and byte-array fields remain opaque. The reference
path/revision and schema files are listed under
`mutagen-vmad-oracle` in `sources.lock.json`; no reference implementation code is
copied into the Rust library.

The function metadata is generated into `src/condition/condition_schema.rs` by
`py -3.13 tools/generate-condition-schema.py`. The offline generator requires
the exact locked and clean schema checkout; run `tools/cargo.ps1 fmt --all`
after regeneration.

The pinned COBJ, CMPO, and MISC schemas now also drive a separate raw FormLink
slot ledger. It records each record header and each explicitly typed nested link
with the source subrecord tag, decoded payload offset, record version, plugin
hash, listed-master count, and raw FormID bit-pattern candidates. COBJ coverage
includes output/workbench/art/sound/category/component links and CTDA reference
fields. CTDA comparison values are included only when the global encoding flag
is set, and parameters only when the pinned function map explicitly says the
slot is a FormLink. Unmapped and defaulted slots remain untouched raw words.
CMPO includes crafting sound, scrap item and scalar links; MISC includes its
transform, sound, keyword, featured-message and component links. GLOB contributes
its record header; scalar bytes remain separate evidence.

The frozen corpus contains 4,611 physical recipes across 16 plugin files. The
audit decoded 14,916 component entries, 1,612 category links, 4,611 output-count
entries, and 3,626 CTDA condition subrecords; every condition is a complete
32-byte structure and no CIS1/CIS2 companions occurred. It found 1,133 records
with a BNAM field and 4,576 with a CNAM field; every observed field value was
nonzero. These are physical candidates, including overrides and plugins that
may not be active. Detailed record rows remain in ignored
`local/workshop-recipe-007`.

The COBJ record-version histogram is 131: 4,364; 118: 33; 116: 38; 115: 12;
112: 94; 111: 54; 110: 7; and one each at 109, 106, 103, 102, 100, 97, 86, 83
and 76. `INTV` had a priority value in 4,450 entries and an absent priority in
161 entries. No malformed supported fields or repeated schema-singleton fields
were found.

An isolated Mutagen overlay reader independently parsed all 4,611 records. Its
per-plugin structural histograms matched Rust for record version, component
quantity sequences, category counts, CTDA count and fixed fields (flags, operator,
comparison encoding, function index, and run-on value), created-object/workbench
link presence, and INTV counts/priorities. Source hashes were stable before and
after both scans. The comparison deliberately does not compare resolved numeric
FormLinks: Rust keeps the on-disk values raw, while Mutagen may map them through
its plugin identity resolver.

The independent comparison checks all 479 named functions, all 220 explicit
three-slot mappings, and the 259 named functions whose parameter map defaults.
Function names match for all entries; explicit types/categories match, and every
defaulted function remains unresolved in Rust despite the reference reader's
`None` fallback. On the physical recipe corpus, 14 observed explicit mappings
match across 3,609 conditions; the other 17 use function index 300
(`IsInInterior`). The two raw CTDA parameter words remain unchanged.

A follow-on physical identity-candidate pass joins every Rust COBJ component
entry to Mutagen's COBJ row by exact source-plugin EDID bytes and component
index. All 14,916 pairs and quantities match. The join is fail-closed for
missing, duplicate, malformed, non-ASCII, or ambiguously terminated EDIDs; its
positive and negative fixtures passed. Across the 16-plugin proof, 15,338
physical `IItem` records form the candidate index, and every component entry
has exactly one physical candidate. The 209 distinct candidate keys point to
CMPO (13,805 references), MISC (753), ALCH (346), AMMO (11), and WEAP (1).
These are physical candidates, not selected overrides or runtime identities.

The raw FVPA number equals Mutagen's local FormKey number in 14,842 rows. In 74
rows the low 24 bits match while the raw high byte is `0x01`; all 74 come from
the two large DLC masters. No selector, master-slot or ESL rule is inferred,
and Rust's raw FormIDs are still unresolved. A separate schema-grounded edge
inventory found 34 CMPO scrap-item links, 33 non-null CMPO scalar-global links,
and 1,012 MISC CVPA component/count links; each link has one physical target
candidate. One CMPO has a null scalar link. This does not establish item yield,
inventory conversion, scrap amounts or gameplay behavior. The Rust audit parses
CMPO `DATA`/`CUSD`/`MNAM`/`GNAM`, MISC `PTRN`/`YNAM`/`ZNAM`/`KWDA`/`FIMD`/`CVPA`/`CDIX`, and GLOB `FNAM`/`FLTV`, while
keeping links and value bytes raw. Malformed supported widths fail; unknown GLOB
type bytes stay opaque with a diagnostic. A strict same-plugin EDID comparison
against Mutagen matched all 34 CMPO records and all 3,651 MISC records; record
local-ID candidates matched 34/34 and 3,651/3,651, all 34 AutoCalcValue
sequences, 34 scrap-item links, 33 non-null scalar links, all 1,012 CVPA
local-ID/count pairs, and all CDIX byte lists. Those numeric checks are
observational local-ID comparisons only; no runtime identity or conversion rule
is assigned. The scan also found 29 CDIX bytes and 1,943 GLOB records with one
FLTV payload each. A second pinned-schema comparison joined all 1,943 GLOB
records: all header local-ID candidates and value presence/widths matched; 784
explicit FNAM bytes matched, while 1,159 records omit the default float
discriminator. The raw FLTV numeric bits remain uncompared and uninterpreted.
The oracle also writes `scrap-scalar-raw-values.jsonl`, attaching the exact raw
FNAM/FLTV bytes and payload hashes to each non-null CMPO scalar link. All 33
links have one physical GLOB candidate across five distinct FormKeys; their
Mutagen type candidates are all `f`. This file is candidate evidence for later
work and makes no assertion about runtime selection or scrap scaling.
Detailed rows and hashes are in ignored `local/workshop-recipe-018` and
`local/workshop-oracle-027`, summarized in `reports/workshop-checkpoint.json`.
The latest audit emitted 50,303 FormID rows, including 40,064 nested slots
across 8,296 COBJ/CMPO/MISC records. The independent reader joined all 8,296
records and matched 37,699 non-null local-ID candidates; 2,365 raw zero slots
were retained. Malformed-width tests cover new CUSD, PTRN, KWDA and FIMD fields.
A ledger regression fixture pins the CTDA global comparison at +4, explicitly
mapped FormLink parameters at +12 and +16, and `Reference` at +24. It excludes
numeric and defaulted parameter slots.

Three conditions are emitted by the pinned Mutagen Fallout 4 writer in a minimal
COBJ plugin and read back by Mutagen. Rust tests consume the exact 32-byte CTDA
payloads and the full plugin. The float and global variants cover packed
flags/operator, float bits versus global FormLink comparison encoding, the
`GetStageDone` record and number parameters, the function's unknown word,
run-on/reference fields, and trailing signed unknown words. A
`GetVMScriptVariable` condition verifies the writer's `CIS1`/`CIS2` companions
and the serialized order `CTDA, CTDA, CTDA, CIS1, CIS2`; Rust retains that flat
sequence and both exact companion payloads without assigning companion linkage
or condition behavior. The FormLinks point to records in this synthetic plugin;
this checks serialization and parsing only, and assigns no retail identity.
In-workspace fixtures live in `tests/fixtures/condition/`, including the small
plugin, the three CTDA payloads and both CIS payloads. Generated writer output
and the receipt stay under ignored `local/`.

Rebuild the pinned reference and generate a fresh writer/reader receipt with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-workshop-oracle.ps1
dotnet .\local\workshop-oracle-bin\workshop-oracle.dll --write-condition-fixture local/workshop-condition-fixture-new
```

Reproduce the Rust audit with a fresh evidence directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bin audit-workshop
.\local\target\debug\audit-workshop.exe 'G:\SteamLibrary\steamapps\common\Fallout 4' local/proof-fo4-002 local/workshop-recipe-new
```

Build and run the isolated comparison with a separate fresh output directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-workshop-oracle.ps1
dotnet .\local\workshop-oracle-bin\workshop-oracle.dll 'G:\SteamLibrary\steamapps\common\Fallout 4' local/proof-fo4-002 local/workshop-recipe-new local/workshop-oracle-new
```

Both tools require a new output directory under ignored `local/`. The Rust audit
checks each direct `Data` plugin against the completed proof before scanning and
re-hashes it afterward. Completion markers are written last. The Mutagen oracle
is a separate research executable and is never linked by the Rust application.
Its build succeeds at the pinned revision; NuGet reports an upstream `NU1902`
warning for `Microsoft.Build.Tasks.Git` 10.0.300. No Mutagen source was changed.

This work does not resolve active plugin order, overrides, ESL slots, ingredient
names, perks or inventory. It does not resolve raw CTDA/FormLink IDs to runtime
identities or evaluate conditions, recipe eligibility, consume/refund materials, validate settlement placement,
commit/cancel a build, or simulate workshop power/settlers/production. Those
remain separate Fallout 4 implementation gates.
