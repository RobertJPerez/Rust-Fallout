# Missing-script source trace

`trace-missing` takes an existing census and locates each absent script in original
plugin records. It uses the census observer API to reuse the existing record and
VMAD decoders. It does not create an active plugin list or choose override winners.

```powershell
.\target\debug\skyrim-prep.exe trace-missing --census local\census-new.json --output local\trace-new.json
```

Every plugin is hashed against the census. Master lists are parsed from those
verified sources, rather than trusted from input JSON. A second pass verifies the
same hashes before collecting same-origin definitions and direct reference edges.
This pass uses the shared selective record reader. CELL/WRLD ancestry follows
the shared reader's group extents; no separate ESM framing/decompression exists.

The report preserves source headers, editor-ID bytes, containing-cell/world IDs,
deletion and placed-reference disabled flags, VMAD presence, CELL flag bytes,
attachment status and offsets, and the raw bytes of link fields. It keeps duplicate
edges and distinct physical source records. Candidate keys use normalized origin
plugin plus local ID only within this Skyrim trace; they are not the shared runtime
`FormKey` and do not introduce a Skyrim `ProfileId`. A record owned by a light
plugin can use its own ESL local ID when its HEDR version validates that range.
Cross-file `FE` references remain unresolved without a verified light-slot map.
The current trace command still makes no runtime-ID conversion. The separate
`map-profile` command can compute full/light slots only from a caller-supplied
order; see the [explicit profile mapping contract](profile-mapping.md).
Missing target names that cannot be located remain explicit findings, as do
undecoded VMAD declarations.

Direct links cover `NAME` and `XESP` in REFR/ACHR, and `EFID` in
SPEL/SCRL/ALCH/INGR/ENCH. The actor-path slice starts from missing-script MGEF
records, follows source spells, then walks NPC `SPLO`/`TPLT`, `LVSP`/`LVLN`
12-byte `LVLO` entries, and placed `ACHR NAME` links in reverse. Raw FormID bytes
and decoded-payload offsets are retained. Unsupported field sizes and unresolved
source master selectors remain findings. It does not apply template inheritance,
leveled selection, runtime winners, script instantiation, quest aliases,
Papyrus-created objects or archive activation. Absent-script targets inherit the
census asset inventory; archives are not rescanned by this command. A connected
path edge whose source record cannot be assigned a source key is retained and
counted as an unresolved actor identity, which makes the command return findings.

On the measured 1.7.104.0 profile, two attached absent-script magic effects have
two source spell candidates. No NPC, template-descendant, leveled spell/NPC list,
or placed-actor path to either spell was found in the ten installed plugins. This
is a source observation, not proof that gameplay cannot create or grant an actor.

The installation census also preserves the first eight bytes of each archived
and loose `Scripts/*.pex` file. It distinguishes Skyrim's big-endian PEX prefix
from other little-endian PEX prefixes, decodes only the corresponding major,
minor, and game fields, and groups the observed tuples. Truncated prefixes,
unexpected magic, and non-Skyrim tuples remain findings. It does not parse later
header fields, strings, objects, instructions, Papyrus types, or runtime
semantics. This probe determines which PEX dialects are physically present
before any shared decoder boundary is considered; it does not make the
FO4-specific decoder accept another dialect.

The pinned independent trace oracle compares selected record identities, editor
IDs, flags, VMAD presence, CELL flags, placed-reference cell containment, the
direct-edge multiset, and typed actor/list fields decoded from all installed
plugins. For retained actor-path links it reconstructs the SPLO/TPLT/LVLO/NAME
bytes and compares the matching edge multiset. It does not certify raw offsets,
world ancestry, active winners, runtime inheritance, leveled selection or gameplay.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/dotnet.ps1 restore tools/trace-oracle/trace-oracle.csproj --locked-mode --disable-parallel
powershell -NoProfile -ExecutionPolicy Bypass -Command "& './tools/dotnet.ps1' build tools/trace-oracle/trace-oracle.csproj --no-restore --disable-build-servers '-m:1'"
.\local\trace-oracle\bin\Debug\net9.0\trace-oracle.exe local\trace-new.json
powershell -NoProfile -ExecutionPolicy Bypass -File tools/test-trace-oracle.ps1 -Trace local\trace-new.json
```

The mutation control flips one direct target FormID bit while keeping record and
edge counts fixed. The checker must find both the missing original edge and the
unexpected replacement. It first requires the unaltered report to pass.

A record in a cell with a name containing `Test` is only a source observation.
An initially-disabled reference can be enabled later. No installed override
candidate means no candidate in this measured corpus, not a guarantee about
other profiles or mods. The next source gate is a verified full/light plugin
profile and load-order mapping; original runtime behavior remains separate.
