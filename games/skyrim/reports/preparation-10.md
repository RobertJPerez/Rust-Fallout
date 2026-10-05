# Preparation checkpoint 10: placed model references

Date: 2026-10-04

This checkpoint adds `trace-placed-uses`, a reverse source-reference pass from
one exact `ACTI` FormID to placed `REFR NAME` fields. It reuses the pinned
`fallout-data` selected-record visitor, digest reader, master-list identity and
`AssetPath`; no shared record framing, decompression or path normalization was
copied. The exporter retains target model and editor-ID bytes, per-plugin
fingerprints, reference offsets, raw links and containing cell/world context.

The target was `Update.esm` `ACTI` `0x01003206`, editor ID
`ccBGS_ARTrigPressurePlate01`, file-local owner `update.esm:0x003206`. Its
`MODL` field normalizes to
`creationclub/_shared/dungeons/ayleidruins/interior/triggers/artrigpressureplate01.nif`.
The target source hash matches checkpoint 09. The read-only scan covered all ten
installed plugin sources and counted 866,250 `REFR` records with 866,240
four-byte `NAME` fields. It found **zero matching placements**, zero malformed
`NAME` fields and zero unresolved name indices in these inputs.

This narrows the previous model trace: the legacy stream-83 NIF is attached to
an ACTI source record and has an archive candidate, but no placed `REFR NAME`
in this installed ten-plugin corpus points to that ACTI identity. This does not
prove the object is unused or inactive. It does not scan temporary references,
actors, leveled spawns, Papyrus-created objects or absent optional content; it
does not apply load order, select a winning override or establish runtime asset
consumption. The installed profile remains SE with four included Creation
plugin/archive pairs, not the complete paid Anniversary Edition corpus.

Synthetic tests verify declared-master ownership (including same local IDs with
different origins), cell ancestry, normalized model paths, and unresolved light
`FE` references without an active order. They also verify plugin-path boundary
validation and that malformed `NAME` fields remain reportable findings. A
synthetic HEDR 1.71 light-plugin case also verifies expanded file-local IDs and
ordinary full-plugin master links; this format fixture is not a claim of full
Anniversary Edition content validation. The installed data remained read-only.
Full test, format, Clippy and build results are recorded in
[`preparation-10.json`](preparation-10.json); the detailed local JSON report is
ignored and listed by hash there. The command contract and its boundaries are in
[`placed-model-uses.md`](../docs/placed-model-uses.md).
