use fallout_data::{obscript_census, plugin};
use flate2::{Compression, write::ZlibEncoder};
use std::{fs, io::Write};

fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}

fn record(kind: &[u8; 4], flags: u32, data: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &0x123u32.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}

fn script_header(bytes: u32, refs: u32) -> Vec<u8> {
    [
        &[0; 4][..],
        &refs.to_le_bytes(),
        &bytes.to_le_bytes(),
        &3u32.to_le_bytes(),
        &[0, 1, 1, 0],
    ]
    .concat()
}

#[test]
fn embedded_script_census_keeps_body_provenance_and_never_reuses_a_header() {
    let source = tempfile::tempdir().unwrap();
    let path = source.path().join("OriginalFixture.esm");
    let mut bytes = record(b"TES4", 0, &field(b"HEDR", &[0; 12]));
    let record_offset = bytes.len() as u64;
    let mut body = field(b"SCHR", &script_header(4, 7));
    body.extend(field(b"SCDA", &[0x1e, 0, 0, 0]));
    // This second body must not borrow the first body's declared size/ref count.
    body.extend(field(b"SCDA", &[0x1d, 0, 0, 0]));
    body.extend(field(b"SCHR", &script_header(10, 8)));
    body.extend(field(b"SCDA", &[0x10, 0, 6, 0, 3, 0, 9, 0, 0, 0]));
    bytes.extend(record(b"INFO", 0, &body));
    // Focused scope defers this malformed unrelated payload explicitly.
    bytes.extend(record(b"LAND", 0, &[0xba, 0xd0]));
    fs::write(&path, &bytes).unwrap();
    let mut observed = Vec::new();
    let report = obscript_census::inspect(&path, true, |site, bytes, program| {
        observed.push((
            site.scda_data_decoded_offset,
            bytes.len(),
            program.instructions.len(),
        ));
        Ok(())
    })
    .unwrap();
    assert_eq!(report.counts.compiled_bodies, 3);
    assert_eq!(report.counts.compiled_bytes, 18);
    assert_eq!(report.counts.instructions, 3);
    assert_eq!(report.counts.event_ids.get(&3), Some(&1));
    assert_eq!(report.record_payloads_decoded, 2);
    assert_eq!(report.record_payloads_deferred, 1);
    assert_eq!(report.bodies_with_issues, 1);
    assert_eq!(report.bodies[0].declared_references, Some(7));
    assert_eq!(report.bodies[1].declared_references, None);
    assert_eq!(report.bodies[2].declared_references, Some(8));
    assert_eq!(report.bodies[2].script_type, Some(0x100));
    assert!(
        report
            .bodies
            .iter()
            .all(|b| b.site.record_file_offset == record_offset)
    );
    assert_eq!(observed, [(32, 4, 1), (42, 4, 1), (78, 10, 1)]);
    assert!(!report.execution_ready);
    assert!(!report.retail_parity_accepted);
    assert!(obscript_census::inspect(&path, false, |_, _, _| Ok(())).is_err());
}

#[test]
fn focused_script_reads_keep_strict_compression_and_report_metadata_failures() {
    let source = tempfile::tempdir().unwrap();
    let path = source.path().join("OriginalFixture.esm");
    let mut body = field(b"SCHR", &script_header(99, 0));
    body.extend(field(b"SCDA", &[0x1e, 0, 0, 0]));
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
    compressor.write_all(&body).unwrap();
    let mut packed = (body.len() as u32).to_le_bytes().to_vec();
    packed.extend(compressor.finish().unwrap());
    let mut bytes = record(b"TES4", 0, &[]);
    bytes.extend(record(b"SCPT", plugin::COMPRESSED, &packed));
    fs::write(&path, &bytes).unwrap();
    let report = obscript_census::inspect(&path, true, |_, _, _| Ok(())).unwrap();
    assert_eq!(report.bodies_with_issues, 1);
    assert_eq!(
        report.bodies[0].issues,
        ["SCHR compiled size differs from SCDA extent"]
    );
    assert_eq!(report.bodies[0].instructions, Some(1));
    *bytes.last_mut().unwrap() ^= 0xff;
    fs::write(&path, &bytes).unwrap();
    let error = obscript_census::inspect(&path, true, |_, _, _| Ok(())).unwrap_err();
    assert!(
        error.to_string().contains("strict integrity check failed"),
        "{error}"
    );
}
