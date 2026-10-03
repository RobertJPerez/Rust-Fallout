use fallout_data::{plugin, script_bindings};
use flate2::{Compression, write::ZlibEncoder};
use sha2::{Digest, Sha256};
use std::{fs, io::Write};

fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}

fn record(kind: &[u8; 4], flags: u32, id: u32, data: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}

fn header(refs: u32, bytes: u32, variables: u32) -> Vec<u8> {
    [
        &[0x5a; 4][..],
        &refs.to_le_bytes(),
        &bytes.to_le_bytes(),
        &variables.to_le_bytes(),
        &[0, 1, 0x34, 0x12],
    ]
    .concat()
}

fn plugin_header() -> Vec<u8> {
    let hedr = [1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat();
    let fields = [
        field(b"HEDR", &hedr),
        field(b"MAST", b"Base.esm\0"),
        field(b"DATA", &[0; 8]),
    ]
    .concat();
    record(b"TES4", 0, 0, &fields)
}

fn source(raw_form: u32) -> Vec<u8> {
    let calls = [
        0x1c, 0, 1, 0, 0x01, 0x10, 0, 0, 0x1c, 0, 2, 0, 0x01, 0x10, 0, 0,
    ];
    let mut declaration = [0; 24];
    declaration[..4].copy_from_slice(&42_u32.to_le_bytes());
    let fields = [
        field(b"SCHR", &header(2, calls.len() as u32, 99)),
        field(b"SCDA", &calls),
        field(b"SLSD", &declaration),
        field(b"SCVR", b"ref_\xe9\0"),
        field(b"SCRO", &raw_form.to_le_bytes()),
        field(b"SCRV", &42_u32.to_le_bytes()),
        // This second embedded unit has stale metadata and no compiled body.
        field(b"SCHR", &header(1, 0, 0)),
    ]
    .concat();
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
    compressor.write_all(&fields).unwrap();
    let packed = [
        (fields.len() as u32).to_le_bytes().as_slice(),
        &compressor.finish().unwrap(),
    ]
    .concat();
    [
        plugin_header(),
        record(b"INFO", plugin::COMPRESSED, 0x01000123, &packed),
        record(b"LAND", 0, 0x01000124, &[0xba, 0xd0]),
    ]
    .concat()
}

#[test]
fn source_less_embedded_bindings_preserve_stale_metadata_and_stable_form_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Fixture.esm");
    fs::write(&path, source(0x01001234)).unwrap();
    let report = script_bindings::inspect(&path, true, |_, units| {
        assert_eq!(units.len(), 2);
        assert!(units[0].source.is_none());
        assert_eq!(units[0].variables[0].name.data, b"ref_\xe9\0");
        assert_eq!(units[0].flags(), 0x1234);
        Ok(())
    })
    .unwrap();
    assert_eq!(report.counts.units, 2);
    assert_eq!(report.counts.reference_calls, 2);
    assert_eq!(report.counts.calls_to_forms, 1);
    assert_eq!(report.counts.calls_to_variables, 1);
    assert_eq!(report.record_payloads_deferred, 1);
    assert_eq!(report.units_with_issues, 1);
    assert!(report.units[0].issues.is_empty());
    assert_eq!(report.units[0].declared_variables, 99);
    assert_eq!(report.units[0].max_variable_index, Some(42));
    assert_eq!(
        report.units[1].issues,
        ["SCHR reference count differs from ordered table length"]
    );
    let tuples: Vec<u8> = [
        0_u32.to_le_bytes().as_slice(),
        &1_u16.to_le_bytes(),
        &[0],
        &0x01001234_u32.to_le_bytes(),
        &8_u32.to_le_bytes(),
        &2_u16.to_le_bytes(),
        &[1],
        &42_u32.to_le_bytes(),
    ]
    .concat();
    assert_eq!(
        report.units[0].caller_bindings_sha256,
        format!("{:x}", Sha256::digest(tuples))
    );
    let stable = &report.units[0].stable_form_keys_sha256;
    // Noncanonical self selectors bind to the same stable identity, while exact
    // source and raw caller hashes must reflect the different authored bytes.
    fs::write(&path, source(0x05001234)).unwrap();
    let rebased = script_bindings::inspect(&path, true, |_, _| Ok(())).unwrap();
    assert_eq!(stable, &rebased.units[0].stable_form_keys_sha256);
    assert_ne!(report.source_sha256, rebased.source_sha256);
    assert_ne!(
        report.units[0].metadata_sha256,
        rebased.units[0].metadata_sha256
    );
    assert_ne!(
        report.units[0].caller_bindings_sha256,
        rebased.units[0].caller_bindings_sha256
    );
    assert!(!report.execution_ready);
    assert!(!report.retail_parity_accepted);
    assert!(script_bindings::inspect(&path, false, |_, _| Ok(())).is_err());
}

#[test]
fn zero_out_of_range_and_missing_local_bindings_stay_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Fixture.esm");
    let calls = [
        0x1c, 0, 0, 0, 0x01, 0x10, 0, 0, 0x1c, 0, 2, 0, 0x01, 0x10, 0, 0,
    ];
    let body = [
        field(b"SCHR", &header(1, calls.len() as u32, 0)),
        field(b"SCDA", &calls),
        field(b"SCRV", &77_u32.to_le_bytes()),
    ]
    .concat();
    fs::write(
        &path,
        [plugin_header(), record(b"SCPT", 0, 0x01000123, &body)].concat(),
    )
    .unwrap();
    let report = script_bindings::inspect(&path, true, |_, _| Ok(())).unwrap();
    assert_eq!(report.units[0].issues.len(), 3);
    assert_eq!(report.counts.reference_calls, 2);
    assert_eq!(
        report.counts.calls_to_forms + report.counts.calls_to_variables,
        0
    );
    assert!(report.units[0].issues[0].contains("SCRV index 77"));
    assert!(report.units[0].issues[1].contains("caller reference 0"));
    assert!(report.units[0].issues[2].contains("caller reference 2"));
    // An observer failure must prevent a complete report from being returned.
    assert!(
        script_bindings::inspect(&path, true, |_, _| Err(fallout_data::Error::Unsupported(
            "fixture observer".into()
        )))
        .is_err()
    );
    let mut damaged = source(0x01001234);
    // Corrupt the Adler32 of the INFO compressed stream, before the LAND record.
    let checksum = damaged.len() - record(b"LAND", 0, 0x01000124, &[0xba, 0xd0]).len() - 1;
    damaged[checksum] ^= 0xff;
    fs::write(&path, damaged).unwrap();
    assert!(
        script_bindings::inspect(&path, true, |_, _| Ok(()))
            .unwrap_err()
            .to_string()
            .contains("strict integrity check failed")
    );
}
