# Skyrim landscape links and cell context

`trace-landscape-links` exports a source-only chain from `LAND` layer headers to
`LTEX` records and from each land texture to its `TXST`, `MATT`, and `GRAS`
records. It also emits physical `CELL`, `WRLD`, `REGN`, and selected cell
context target identities used by LAND group context and CELL FormID links. It uses the shared TES record
visitors and the existing owner-aware
Skyrim `SourceKey` resolver. It does not implement a second record reader.

```powershell
.\target\debug\skyrim-prep.exe trace-landscape-links `
  --data 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data' `
  --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim.esm' `
  --plugin 'G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Update.esm' `
  --output 'local\landscape-links.jsonl'
```

The structural links follow the pinned Skyrim record definitions in
`sources.lock.json`: `LAND` `BTXT` and `ATXT` payloads are checked as eight-byte
headers containing a texture FormID, quadrant byte, auxiliary byte, and signed
layer index. `LTEX` `TNAM`, `MNAM`, and repeated `GNAM` fields are checked as
four-byte links to `TXST`, `MATT`, and `GRAS`. The full field digest, payload
offset, raw FormID, and link status are emitted. The quadrant and auxiliary
bytes are retained as data; the scan does not infer their runtime meaning.

Regular FormIDs resolve through the referring file's declared master list.
Physical records owned by light plugins keep a local key only when their HEDR
version validates it. Cross-file FE references stay unresolved until an
explicit active profile is available. When a link resolves to a key, the report
emits each physical record once and gives each link a candidate count for the
matching target kind and source key. It never chooses an override or winner. A
key with no scanned candidate is reported separately;
that result does not establish that the target is invalid or absent from the
full installed content set.

Each `LAND` row also carries its raw ancestor group frames as supplied by the
shared reader: byte offset, extent, numeric kind, four label bytes, and trailing
header bytes. Group kind 6 and kind 1 labels are retained as raw containing-cell
and worldspace FormIDs, resolved through the referring plugin's declared
masters, and joined to physical `CELL` and `WRLD` candidates. The report keeps
candidate counts and does not choose an override. Cross-file light references
remain unresolved without an explicit profile. The other group labels stay
opaque. LAND rows are joined to physical CELL records by source key without
choosing an override.

For each `CELL`, the trace reads subrecord framing and structurally inspects
`XCLC`. The pinned layout accepts signed 32-bit X/Y grid coordinates in an
eight-byte payload, or those coordinates followed by one flags byte and three
reserved bytes in a twelve-byte payload. The full payload length, hash, and a
raw preview are kept; unsupported lengths stay visible without guessed fields.
Repeated `XCLC` fields and cells without one are counted. These are raw cell
grid integers: the trace does not convert them to world units, infer streaming
or persistence behavior, or map LAND vertices to positions. Other CELL fields
remain opaque, and WRLD bodies are not visited.

The trace also retains `XCLW` as an optional four-byte IEEE 754 single-precision
field. It records the exact bits and hash, classifies finite values, infinities,
and NaNs, and emits a JSON number only for finite values; unsupported payload
lengths remain raw findings. This avoids losing unusual bit patterns and keeps
the scan separate from runtime water selection or interior-cell rules. Absence
is reported as absence; no WRLD default is synthesized.

`XCLR` is retained as a whole-field hash, exact length, and bounded preview.
Complete four-byte entries are emitted as CELL-to-REGN FormID links through the
existing owner-aware resolver. The trace preserves each entry's offset, bytes
hash, and candidate count without choosing an override. If the payload ends
with one to three bytes, complete entries remain available and the trailing
bytes are emitted as a separate malformed field; they are never padded or
discarded. Region record bodies and runtime region selection remain opaque.
Across all supplied plugins, entry parsing is capped at one million links.

The Skyrim CELL context adapter also recognizes `XCCM` (Sky/Weather from
Region) as a `REGN` link and `XLCN` (Location) as an `LCTN` link, plus `XCWT` to
`WATR`, `XCIM` to `IMGS`, `XEZN` to `ECZN`, `XCAS` to `ASPC`, `XCMO` to
`MUSC`, and `LTMP` to `LGTM`. Each field must be exactly four bytes; the
existing resolver emits the target source key and physical candidate count
without selecting an override. Other field sizes are retained as malformed raw
evidence. `LCTN` `PNAM` is also read as a four-byte parent-location link to
`LCTN` through the same resolver. The physical parent candidate count is
reported without selecting an override or recursively constructing a winning
hierarchy. Other known LCTN fields retain hashes, lengths and bounded raw bytes;
unknown tags remain visible. No runtime location containment, weather, water,
image-space, encounter, acoustic, music, or lighting behavior is evaluated. The
pinned schema identifies the FormLink fields, while the Skyrim-specific XCCM
target remains distinct from cross-game climate naming.

CELL `XWEM` is identified by the pinned schema as a string. The installed
source corpus records NUL-terminated values rooted at case-insensitive
`Data\\Textures\\...` paths; the adapter retains each exact field length,
whole-field hash, and bounded raw bytes, requires the terminator and
`Data\\`/`Data/` root, strips only that root,
and sends the remaining path through the shared `AssetPath` normalizer. It
checks loose files and immediate BSA name indexes through the shared lookup.
Missing terminators, unexpected roots, invalid paths, nonzero bytes after the
terminator, absent candidates, and archive-index failures remain visible. The
lookup reads no asset payload, decompresses no archive member, selects no
winner, and infers no water or rendering behavior.

`XCLL` fields are also attached to their physical CELL rows with full hashes,
exact lengths, and bounded raw previews. The installed corpus has 64-byte and
92-byte forms; those lengths are identified, but no lighting members are
decoded. Other sizes remain visible as unprofiled. The field profile is not a
lighting or rendering interpretation.

The completion trailer also groups the visited `CELL`, `LAND`, and `LTEX`
fields by tag, occurrence count, and exact payload length. For each `LAND` tag
it also reports how many records have zero, one, or more occurrences. The
per-record counts keep sparse fields and repeated vertex-texture chunks visible
instead of inferring their distribution from global totals. These profiles help
bound and validate future Skyrim field adapters; they do not decode opaque
payloads or assign semantics.

To continue the graph to texture paths, join `TXST` rows by their physical
`plugin_file` and `form_id` to `trace-texture-sets` output. That command already
normalizes texture-root-relative slot paths and checks loose files and indexed
BSA names. This landscape trace does not read texture payloads, archive entries,
terrain height/normal data, vertex paint, alpha weights, material payloads, or
runtime state.

The scan is bounded to 300,000 selected records across `LAND`, `LTEX`, `TXST`,
`MATT`, `GRAS`, `CELL`, `WRLD`, `REGN`, `CLMT`, `WATR`, `IMGS`, `ECZN`,
`ASPC`, `MUSC`, and `LGTM`; one million visited LAND/LTEX/LCTN subrecords;
and two million visited CELL subrecords across the supplied plugins.
XWEM strings over 4,096 bytes remain bounded as explicit findings. Full-field
SHA-256 and exact payload lengths are retained; raw previews are capped at 32
bytes per field, with XWEM string bytes capped at 4,096. CELL `XCLC`, `XCLW`, complete `XCLR` entries, and
the seven listed four-byte context links are structurally inspected, while
`XCLL` is retained as raw lighting evidence.
Other `LAND`, `LTEX`, `TXST`, `MATT`,
and `GRAS` data stays opaque, including unknown subrecords. No output status
certifies terrain rendering or gameplay.
