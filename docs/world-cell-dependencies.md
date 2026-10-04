# Winning CELL source dependencies

`world::dependencies::inspect_cell_key(&mut RecordStore, &FormKey, Limits)` builds
a bounded source report for one exact, nondeleted winning NV CELL. The existing
`cell` inspector consumes it with `--include-dependencies`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File G:\Rust-Fallout\tools\cargo.ps1 run --locked --jobs 2 -p fallout-cli --bin fallout -- --output local\cell-dependencies.json cell --install "G:\SteamLibrary\steamapps\common\Fallout New Vegas" --load-order profiles\nv-inspection-order.json --editor-id GSDocMitchellHouse --defer-unread-payloads --include-dependencies
```

During team work, run Cargo through the assigned automatic focused-slot wrapper.
Use a fresh private report path. The flag adds `dependency_report` to the existing
CELL JSON. Without it, the default report schema and bytes remain unchanged.
The optional graph finishes before the existing cell/model inspection runs;
either inspection can reject the request before a report is emitted. These graph
limits apply to this producer; the original inspector retains its existing limits.

## Exact source scope

Membership uses each definition's **winning** parent CELL label, resolved with
that winning source's master table. Moved overrides belong to their new cell.
Deleted winners remain header-only tombstones. Older bodies are never used as
fallbacks. Root members include placements and other record kinds, in supplied
source order and physical winning-header order.

The producer reuses the existing CELL and generic placement decoders and the
existing terrain WRLD decoder. It follows root CELL `group.world`, WRLD `WNAM`,
and generic placement `XESP`/`XTEL` links. WRLD climate, water, image-space,
encounter-zone and music links retain typed terminal identities. Placement `NAME`
targets are terminal base identities; NPC/CREA extras belong to the actor lane.
Recognized projectile enable parents remain header-only terminals.

A linked placement outside the root cell may be decoded by the same generic
decoder. Its `group.cell` edge identifies its source CELL as a header-only
terminal. The producer does not open that cell's children. LAND, NAVM and other
root child kinds also retain headers and source group context without body
validation. Unhandled fields and dependencies remain explicitly unverified.

Every edge retains its source ordinal/header offset, raw form ID, role, expected
kinds and the existing resolver's resolved, null, missing, deleted, wrong-kind or
unimplemented player-binding status. Existing winners with a wrong kind remain
header-only evidence. Missing and runtime-only identities have no manufactured
record node. Deleted or wrongly typed root identities reject the request.

Expanded nodes retain exact decoded bodies, SHA-256 hashes, typed projections and
all physical field sites. `field_sites.decoded_header_offset` identifies the
four-byte tag. `span.decoded_offset` identifies the first data byte six bytes
after that header; `span.bytes` is the exact data length. An `XXXX` prefix remains
in the raw body while the following field's header and data offsets remain exact.
The reused typed projections preserve their existing header-offset convention.
Group-context and member edges have no payload span. The index exposes raw group
ancestry, not absolute group-header offsets; none are guessed.

Nodes sort by supplied source ordinal then physical winning-header offset. Edges
sort by source ordinal/header offset, then data offset for fields. Context edges
have no field offset and use a stable role tie break; this does not claim physical
ordering between unavailable group-header positions. Cycles are retained as node
indices over the `WNAM`/`XESP`/`XTEL` link projection. Membership and parent-context
backlinks are excluded from that projection. Shared targets alone are not cycles.
No inheritance, enablement or teleport rule is applied.

## Source identity and bounds

Ordered source receipts come from the same retained, write-denying source handles
used by `RecordStore`. Every node references one of those receipts by ordinal.
The cohort SHA-256 input is the literal `nv-world-source-cohort-v1` followed by NUL,
then, for each source: ordinal as little-endian u64, source-name byte length as
little-endian u64, exact source-name UTF-8 bytes, source length as little-endian
u64, and the 64 ASCII bytes of its SHA-256. Changing order, names or bytes changes
the identity. This is source evidence, not persistent runtime or save identity.

The fixed ceilings below may be lowered independently. Raising any value fails.

| Limit | Ceiling |
| --- | ---: |
| Winning metadata entries scanned | 1,000,000 |
| Source plugins | 256 |
| Nodes | 4,096 |
| Edges | 16,384 |
| Each requested stored/decoded body | 4 MiB |
| Aggregate retained decoded bodies | 64 MiB |
| Physical field sites | 262,144 |
| Conservative retained metadata estimate | 32 MiB |

Admission precedes source receipt collection, member/key collection, node/edge
clones, field-site collection and typed decoding. Temporary group-key resolution
is also checked against remaining metadata allowance. Requested reads receive the
smaller of the per-record ceiling and remaining aggregate decoded allowance via
the existing strict `read_bounded` method. Stored extents and declared compressed
lengths are checked by that reader before their allocations. An invalid requested
body, checksum, field shape or budget fails the entire request. A successful
result does not exist on failure; a retry uses a fresh request-local builder.

The metadata count includes conservative allowances for cloned identities, field
storage and graph work plus decoder-owned byte copies. It excludes allocator
overhead, index memory already owned by the store and unrelated consumer work;
it is not a process peak-memory promise. Returned reports own their bytes. Source
locks stay with the store and release when the store drops, even after failure.

## Verification and limits of the evidence

`world_dependencies` authored source tests cover exact winning membership, moved
and deleted overrides, source/master/self-selector identity, physical field and
header/data spans including `XXXX`, raw unknown bytes, all link status classes,
world/placement cycles, header-only terminals, every limit, strict compressed
rejection, retries, handle release, cold/warm metadata parity and byte-identical
legacy cell reports. The private handoff records independent byte-framing and
graph comparisons against frozen production CLI outputs and altered-report
rejections; source bytes and reports remain outside Git.

Every report sets `runtime_ready=false`. It does not establish retail inheritance,
activation, destination-cell behavior, archive precedence, actor preparation,
streaming, collision, navigation, input or gameplay acceptance. Source decoding
and the existing preview remain separate from authoritative simulation state.
