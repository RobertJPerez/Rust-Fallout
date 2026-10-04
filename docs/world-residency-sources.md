# Cell residency source inspection

`fallout cell-residency-sources --install INSTALL --load-order ORDER.json
--cell FalloutNV.esm:103df9` runs the existing sealed model and external texture
source plans through `CellResidency`, `ResourceJobs` and the existing importer.
Optional `--index-cache` uses the existing plugin header cache; `--cache` uses a
separate decoded-member cache outside the source installation. Global `--output`
writes the report through the existing protected output boundary.

The report retains exact plugin/order/model/archive hashes, original texture
paths and lookup candidates, decoded texture byte hashes, source quotas and
residency snapshot. It never reports dependency/GPU/collision/behavior readiness.
`captured_sources_available` means complete model and captured external texture
coverage; unknown NIF structure, shader/DDS interpretation, runtime context,
original lookup precedence and gameplay acceptance remain separate.

Missing model or missing/absolute/ambiguous texture inputs produce a report and
nonzero exit without an invented winner, path rewrite or replacement image.
Planning/extraction errors are retained when a model plan was already sealed.
`--source-timeout-ms` bounds each worker polling stage (1..120000, default30000);
existing planning/indexing/fingerprinting limits are separate. A polling deadline
cancels the owned cell request; it never marks the interrupted batch complete.

Authored CLI tests exercise the actual source importer, cache and byte leases,
with unchanged source fingerprints and explicit refusal metadata. Original
installation material and full source reports remain local.

The frozen source inspector was also run on the original Doc Mitchell house
(`FalloutNV.esm:103df9`) with the explicit ten-plugin official order. It decoded
207 model payloads and 312 texture payloads (58,415,256 texture bytes), retaining
all 854 external texture usages. All captured texture paths resolved uniquely.
Every captured plugin and archive fingerprint was unchanged after inspection.

The command correctly exited nonzero: 12 source bases have no model path, so
complete model coverage remains false. This does not establish that every such
record requires a model. Unsupported NIF blocks in 181 models also keep reference
coverage unverified. Dependencies, rendering, collision and behavior remain
Pending; no original game process was launched. The source check supports this
consumer's engineering acceptance, while a selected real absolute/missing/
ambiguous texture failure and original engine handling remain open.
