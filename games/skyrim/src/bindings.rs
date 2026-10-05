//! Streaming, source-addressed VMAD evidence for independent readers and linking.
use crate::{Result, vmad};
use fallout_data::{
    baseline::{digest_reader, open_source},
    plugin::{self, Event},
};
use serde::Serialize;
use std::{
    io::{BufReader, Seek, SeekFrom, Write},
    path::Path,
};

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub bindings: u64,
    pub decoded_tails: u64,
    pub failures: u64,
}

fn line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// One source header, one row per physical VMAD, and a completeness trailer.
/// Offsets into decompressed payloads are not claimed to be physical file offsets.
/// IDs remain file-local; this does not select active plugins or winning records.
pub fn export(path: &Path, writer: &mut impl Write) -> Result<Summary> {
    let name = path.display().to_string();
    let mut source = BufReader::new(open_source(path)?);
    let (bytes, sha256) = digest_reader(&mut source)?;
    source.seek(SeekFrom::Start(0))?;
    line(
        writer,
        &serde_json::json!({
            "type": "source", "schema_version": 1,
            "file": path.file_name().and_then(|s| s.to_str()), "bytes": bytes, "sha256": sha256,
            "identity": "physical records; file-local FormIDs; no override or load-order resolution"
        }),
    )?;
    let mut summary = Summary::default();
    plugin::visit(
        &mut source,
        bytes,
        &name,
        plugin::Limits::default(),
        |event| {
            let Event::Record(record) = event else {
                return Ok(());
            };
            plugin::visit_subrecords(record, &name, |sub| {
                if sub.kind != *b"VMAD" {
                    return Ok(());
                }
                summary.bindings += 1;
                let (_, digest) = digest_reader(&mut std::io::Cursor::new(sub.data))
                    .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?;
                let attachment = match vmad::decode_record(
                    sub.data,
                    record.header.kind,
                    &name,
                    vmad::Limits::default(),
                ) {
                    Ok(value) => {
                        match &value.tail {
                            vmad::Tail::Decoded(_) => summary.decoded_tails += 1,
                            vmad::Tail::Unsupported { .. } => summary.failures += 1,
                            vmad::Tail::Absent => {}
                        }
                        serde_json::json!({
                            "version": value.primary.version,
                            "object_format": value.primary.object_format,
                            "scripts": value.primary.scripts,
                            "tail_offset": value.primary.tail_offset,
                            "tail": value.tail,
                        })
                    }
                    Err(error) => {
                        summary.failures += 1;
                        serde_json::json!({"error": error.to_string(), "raw": sub.data})
                    }
                };
                line(writer, &serde_json::json!({
                "type": "binding", "kind": plugin::signature(record.header.kind),
                "form_id": record.header.form_id, "record_offset": record.header.offset,
                    "record_flags": record.header.flags,
                    "subrecord_offset_in_decoded_record": sub.payload_offset,
                    "vmad_offset_in_decoded_record": sub.payload_offset + 6,
                "vmad_bytes": sub.data.len(), "vmad_sha256": digest, "attachment": attachment,
            })).map_err(|e| fallout_data::Error::Resolution(e.to_string()))
            })
        },
    )?;
    line(
        writer,
        &serde_json::json!({"type": "complete", "summary": summary}),
    )?;
    Ok(summary)
}
