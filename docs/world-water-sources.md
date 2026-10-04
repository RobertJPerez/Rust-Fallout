# Explicit CELL water source requests

`world::water::CellWaterSources::load(store, cell, mounts, limits)` constructs a
private immutable Arc request from one strict winning NV CELL. `receipt()`,
`root()` and `identity()` borrow its evidence. Serialized metadata cannot create
authority. `validate_sources()` checks exact ordered source names/count before
copying receipts, then checks every plugin's bytes and SHA-256.

The producer uses the existing CELL decoder, subrecord visitor, master-context
dependency resolver, protected bounded record store, texture-path policy,
MountIndex, ArchiveInput, resource pool and artifact cache. It introduces no
additional importer, lookup precedence policy or water simulation.

The complete pinned xEdit FNV CELL and WATR definitions were inspected at commit
`9fb016884bec138ea6c7b872cec831537d464c3e`. CELL declares XCLW as a float, XCWT
as a WATR FormID and XNAM as a water noise texture string. `wbCELLAfterLoad`
creates missing XCLW/XNAM when CELL DATA has its water bit. This editor mutation
is excluded: missing physical fields stay absent. WRLD water declarations never
fill CELL inputs.

| Source field | Retained declaration |
| --- | --- |
| XCLW | Exactly four bytes as a u32 float word |
| XCWT | Exactly four raw bytes, canonical master-resolved WATR target and status |
| XNAM | Complete NUL-terminated source framing plus the existing terminator-stripped byte string |

Negative zero, subnormal, sentinel and NaN words remain exact. No f64 conversion,
finite plane, water-height sentinel interpretation or inheritance is performed.
Unknown byte values in paths stay bytes; existing ASCII case/separator policy is
applied only to the separately retained asset lookup path. Complete field framing
includes any validated XXXX prefix. Decoded offsets are distinct from stored-body
offsets; compressed fields never receive a physical frame offset.

A declared XCWT resolves through the winning CELL's own master table. Null,
missing, deleted and wrong-kind target statuses remain explicit. Existing winning
WATR headers retain source plugin/ordinal/hash and canonical identity. Resolved
WATR bodies are read strictly and hashed, but their parameter fields are not
decoded or substituted into CELL declarations. `typed_parameters_included` is
false. Deleted/wrong-kind targets retain header evidence without body reads.

Absent XNAM produces no noise entry. An authored empty string has explicit
`empty-declaration` status. A supported relative string uses existing
`texture_path` and `MountIndex` candidates. No candidate produces an explicit
missing source; unsafe paths and multiple candidates refuse the entire request.
One candidate retains an exact archive/request receipt and the existing protected
ArchiveInput. Its member bytes are not decoded until submitted to resource jobs.
The source status remains `one-archive-source; retail-precedence-unverified`.

`submit_noise(store, jobs, token, cache)` first validates the complete plugin
cohort, token freshness and token source identity. It returns an existing job
handle for one admitted noise member, or `None` for the explicit no-member
statuses. The caller supplies an existing bounded ResourceJobs pool and optional
private cache/source-tree pair. Its token must belong to that pool's controller.
The worker result retains the request Arc, protected mapping and its existing
decoded-byte/outstanding reservation until the artifact is dropped. Cache reuse
uses the existing archive digest/path/transform identity and verified content
hash; no shader or DDS image interpretation is performed here.

| Lowerable construction allowance | Fixed ceiling |
| --- | ---: |
| Ordered plugin sources | 256 |
| Retained CELL/WATR record identities | 2 |
| Visited CELL/WATR fields | 65,536 |
| Each stored/decoded requested body | 4 MiB |
| Aggregate max(stored, decoded) requested bodies | 8 MiB |
| XNAM bytes including terminator | 4,096 |
| Retained raw field framing | 8,192 bytes |
| Conservative retained/temporary metadata | 16 MiB |
| Declared WATR dependency | 1 |
| Selected archive mapping | 1 archive / 16 GiB |
| Planned noise member job | 1 |
| Declared decoded noise bytes | 64 MiB |

Admission precedes source copies, requested reads, path/candidate copies and
mapping. A write-denying source handle spans extent admission and the existing
archive mapping. Stored and decoded lengths remain bounded; failed factories
return no request and release temporary mappings. Metadata estimates exclude the
caller's existing index/mount table and allocator overhead. Plugin/archive
fingerprinting reads are separate from requested-body and member-byte accounting.
Runtime/gameplay readiness stays false.
