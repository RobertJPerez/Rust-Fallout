# Papyrus definitions, operand evidence and attachment binding

The PEX adapter now exposes typed struct members, variables/defaults, properties,
auto-property backing names, accessor indices and state function ranges. It retains
the previously decoded instructions, operands and every source byte. There is no
VM execution or native-host implementation in this slice.

The independent PEX reader exports definition/operand tokens in file order. Rust
compares string bytes as hex and floats as their exact bits, preserving duplicate
string-table indices rather than comparing only their text. All 10,342 physical
files and 269,294 instructions in the completed archive proof match: 4,067,308
semantic tokens. Headers and debug metadata remain outside the value comparison.
The reference converts boolean bytes to bool; the installed values match exactly.

Reproduce the incremental proof without rehashing/reextracting the entire game:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bins
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build-pex-oracle.ps1
.\local\target\debug\verify-pex-corpus.exe local/proof-fo4-002 .\local\pex-oracle-build\Release\fo4-pex-oracle.exe local/pex-values-new
```

The source proof must have a valid completion marker and unchanged census hash.
Every extracted PEX is checked against the frozen source census. All independent
processes must exit successfully; the runner fingerprints its executables before
and after and writes completion last. Private proof: `local/pex-values-001`.
This validates the frozen corpus, not the current activation state or live files.

`link::Catalog` is a physical-definition index. Case-insensitive ASCII binding is
used for script identifiers. Non-ASCII/NUL identifiers produce an explicit result.
Case collisions remain multiple candidates even when two files have identical
bytes. Every class retains archive path, asset path, source hash and object index.

Declaration lookup is derived-first, matching the pinned compiler's member/root
state lookup. It follows only uniquely identified parents, bounds ancestry to
128 definitions and detects cycles. Duplicate local properties/default-state
methods remain ambiguous. Runtime state dispatch, function overload/type checks,
assignment/coercion, inherited script flags and removal behavior remain open.
Backing-variable checks compare auto-property names and declared types within the
owning class; they do not initialize objects or prove native-backed properties.
The class report also audits complete ancestry independently of member lookup,
so finding a local property does not hide a cycle or missing parent farther up.
All 10,342 installed classes reach the unique `ScriptObject` root; the maximum
observed chain contains seven classes. Private final audit: `local/vmad-links-002`.

```powershell
.\local\target\debug\link-vmad.exe local/proof-fo4-002 local/vmad-001 local/vmad-links-new
```

This command rehashes and parses the frozen PEX/VMAD payloads and checks that the
VMAD plugin fingerprints belong to the same source census. It writes private
`classes.json`, streaming `bindings.jsonl` and `complete.json`. Each binding retains
the record origin, VMAD byte range and target PEX byte range when found. Script
entries, properties, quest alias scripts, fragment owners, fragments and scene
phases are inventoried without deciding activation or override winners. Exit 0
means the audit completed; unresolved bindings remain explicit report counts.

The installed physical corpus has 10,342 class definitions with no duplicate class
names and no mismatched/missing auto-property backing declarations. The attachment
audit finds 27,618 unique script entries, 34 missing entries and 20 empty entries;
68,614 resolved top-level properties, 305 missing declarations and 14 properties
on missing scripts; 15,615 resolved fragment/phase functions, 20 missing functions
and 32 on missing scripts. Nested struct members are already decoded/compared but
are not included in the top-level property-binding denominator of 68,933.

All 319 unresolved property rows carry source property flag 1 (edited). They are
not classified as removed. `Data/Scripts` is absent in this installation, so that
standard loose-script directory cannot supply the missing classes. These facts
still do not establish active load order, intended behavior or retail failure.
As a checked example, the Far Harbor holotape quest VMAD names a stage-135 fragment
that is absent from its archived class's seven function declarations. The source
reference and original PEX are retained; no function was synthesized to hide it.

## Call and branch inventory

The static executable inventory reads the same frozen PEX census and verifies
every extracted member against its recorded SHA-256 before indexing or emission.
Each row retains archive, member path, payload hash, class/object, function, code
offset and instruction index. Branches are mapped from relative instruction
displacements to an instruction index or the explicit one-past-end boundary.
That boundary has no assigned runtime/continuation meaning.

The same frozen 10,342-file corpus contains 269,294 instructions. Its opcode
census finds 1,091 `ARRAYLENGTH`, 2,503 `ARRAYGETELEMENT`, 238
`ARRAYSETELEMENT`, 1,455 `STRUCTGET` and 181 `STRUCTSET` instructions. These
counts help prioritize FO4-specific VM work; they do not establish execution,
array/struct value semantics, mutation, aliasing, or gameplay. The complete raw
opcode histogram and source hashes are in
[`reports/executable-checkpoint.json`](../reports/executable-checkpoint.json),
with private rows under `local/pex-executable-003`.

Call operand order is grounded in pinned Caprica source at revision
`e4dee0860914d75e770d3f9ab374f7aba474b701`: `CALLSTATIC` is class, function,
destination; `CALLMETHOD` is function, receiver, destination; and `CALLPARENT` is
function, destination. Rust tests lock those field positions. The static linker
lists exact-name PEX declarations on a physically named class. It does not choose
among states, follow inheritance, validate arity/return types, or claim runtime
dispatch. Method and parent receiver dispatch remain unresolved. Native flags are
reported as declaration candidates, not host API implementations.

## Compiler-authored Fallout 4 fixture

Three Papyrus input scripts in `tests/fixtures/papyrus-compiler` exercise a
global static call, a method call through a typed receiver, a parent call from an
override, a conditional branch, a struct definition/member read/write and array
length/read/write. A minimal `ScriptObject` declaration supplies the only base
type; no Creation Kit install or retail files are needed. Caprica at the exact
revision in `sources.lock.json` compiled these sources in Fallout 4 mode, then
this workspace's Rust CLI parsed all three generated PEX files.

The emitted `CALLSTATIC` operands are class/function/result followed by the two
source arguments; `CALLMETHOD` is function/receiver/result followed by its
integer and float arguments; `CALLPARENT` is function/result followed by its
source argument. The conditional branch targets instruction 4 and its jump
targets the one-past-end boundary at instruction 5. FO4 `STRUCTGET` (opcode 38)
uses destination/struct/member operands and `STRUCTSET` (39) uses
struct/member/value. The sample array code emits `ARRAYLENGTH` (31),
`ARRAYGETELEMENT` (32) and `ARRAYSETELEMENT` (33) with destination/array,
destination/array/index and array/index/value operands. These layouts do not
establish VM mutation or aliasing behavior. The fixture verifier also checks
five emitted calls against their target PEX declarations: static `Int/String`,
method `Int/Float`, parent `Int`, and both conditional-branch static calls.
For identifier arguments it reads the compiled caller parameter/local type;
literal tags supply `Int`, `Float` or `String`. This is a compiler-output
consistency check for these examples, not evidence about compiler rejection
rules, runtime resolution, or VM dispatch. Exact source, compiler, PEX and
parser hashes are recorded in
[`reports/papyrus-compiler-checkpoint.json`](../reports/papyrus-compiler-checkpoint.json);
private compiler output is `local/papyrus-compiler-fixtures-017`.

Reproduce from an x64 Visual Studio developer shell after checking out the
pinned Caprica revision under ignored `local/research/Caprica`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/build_papyrus_reference.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bin fallout4-prep
py -3 tools/verify_papyrus_compiler_fixture.py --output-dir local/papyrus-compiler-fixtures-new
```

The offline C++ compiler is built separately from the Rust app; its package
downloads, registry cache, installed dependencies and build products remain in
ignored `local/`. The checked-in CMake hook supplies the pugixml target alias
expected by that pinned source without changing the research checkout. The
fixture verifies compiler-authored PEX layouts, structural branch targets,
and argument/declaration type consistency for five calls. It does not validate
type/arity rejection rules, VM call dispatch or
continuation behavior, struct mutation/aliasing, native host APIs, scheduling
or persistence.

Reproduce the corpus inventory from a fresh private output directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/cargo.ps1 build --locked --jobs 1 --bin link-pex
.\local\target\debug\link-pex.exe local/proof-fo4-002 local/pex-executable-new
```

The output contains `declarations.json`, source-located `executable.jsonl` rows
for calls and branches, and `complete.json` written last. On the frozen proof,
the inventory covered 10,342 files, 30,727 functions and 269,294 instructions;
it recorded 54,780 branches and 122,952 calls. Of 8,706 `CALLSTATIC` sites, each
has one same-name direct physical declaration candidate; 7,844 of those candidate
declarations carry the native flag. The other 114,186 `CALLMETHOD` and 60
`CALLPARENT` sites remain runtime-unresolved. Evidence and hashes are in
`reports/executable-checkpoint.json`; private output is
`local/pex-executable-002`.
