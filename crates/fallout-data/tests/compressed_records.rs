use fallout_data::{
    compressed_records::{self, Limits},
    plugin,
};
use flate2::{Compression, write::ZlibEncoder};
use sha2::{Digest, Sha256};
use std::{fs, io::Write};

fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn record(kind: &[u8; 4], raw: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn header() -> Vec<u8> {
    record(
        b"TES4",
        0,
        0,
        &field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    )
}
fn compressed(body: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(body).unwrap();
    [
        &(body.len() as u32).to_le_bytes()[..],
        &encoder.finish().unwrap(),
    ]
    .concat()
}

#[test]
fn stored_and_decoded_hashes_include_exact_source_extents_and_ignore_unselected_bodies() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Base.esm");
    let payload = field(b"DATA", &vec![0xe9; 4096]);
    let stored = compressed(&payload);
    let records = [
        record(
            b"MISC",
            0x100,
            0,
            b"unrelated malformed fields stay deferred",
        ),
        record(
            b"LAND",
            0x200,
            plugin::COMPRESSED | plugin::DELETED,
            &stored,
        ),
    ]
    .concat();
    let group = [
        b"GRUP".as_slice(),
        &(records.len() as u32 + 24).to_le_bytes(),
        b"LAND",
        &0_i32.to_le_bytes(),
        &[0; 8],
        &records,
    ]
    .concat();
    let source = [header(), group].concat();
    fs::write(&path, &source).unwrap();
    let report = compressed_records::inspect(&path, Limits::default()).unwrap();
    assert_eq!(report.groups, 1);
    assert_eq!(report.record_payloads_decoded, 2);
    assert_eq!(report.record_payloads_deferred, 1);
    assert_eq!(report.counts.records, 1);
    assert_eq!(report.counts.stored_bytes, stored.len() as u64);
    assert_eq!(report.counts.zlib_bytes, (stored.len() - 4) as u64);
    assert_eq!(report.counts.decoded_bytes, payload.len() as u64);
    let row = &report.rows[0];
    assert_eq!(row.record_flags, plugin::COMPRESSED | plugin::DELETED);
    assert_eq!(row.stored_sha256, format!("{:x}", Sha256::digest(&stored)));
    assert_eq!(
        row.decoded_sha256,
        format!("{:x}", Sha256::digest(&payload))
    );
    assert_eq!(
        report.source_sha256,
        format!("{:x}", Sha256::digest(&source))
    );
    assert!(row.integrity_issue.is_none());
}

#[test]
fn checksum_inspection_retains_taint_and_never_changes_the_strict_default() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Base.esm");
    let payload = b"an independently checkable payload";
    let mut stored = compressed(payload);
    *stored.last_mut().unwrap() ^= 1;
    let source = [
        header(),
        record(b"LAND", 0x200, plugin::COMPRESSED, &stored),
    ]
    .concat();
    fs::write(&path, &source).unwrap();
    assert!(compressed_records::inspect(&path, Limits::default()).is_err());
    let diagnostic = Limits {
        plugin: plugin::Limits {
            inspect_checksum_mismatches: true,
            ..plugin::Limits::default()
        },
        ..Limits::default()
    };
    let report = compressed_records::inspect(&path, diagnostic).unwrap();
    assert_eq!(report.counts.checksum_mismatches, 1);
    let row = &report.rows[0];
    let issue = row.integrity_issue.as_ref().unwrap();
    assert_eq!(issue.file_offset, row.record_file_offset);
    assert_eq!(issue.form_id, 0x200);
    assert_ne!(issue.stored_adler32, issue.calculated_adler32);
    assert_eq!(issue.calculated_adler32, adler2::adler32_slice(payload));
    assert_eq!(row.decoded_sha256, format!("{:x}", Sha256::digest(payload)));
    assert_eq!(fs::read(&path).unwrap(), source);
    assert!(compressed_records::inspect(&path, Limits::default()).is_err());
}

#[test]
fn structural_corruption_surplus_bytes_and_wrong_decoded_extents_are_not_checksum_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Base.esm");
    let valid = compressed(b"hello");
    let mut wrong_size = valid.clone();
    wrong_size[..4].copy_from_slice(&6_u32.to_le_bytes());
    let mut surplus = valid.clone();
    surplus.push(0);
    let mut bad_header = valid.clone();
    bad_header[4] = 0;
    let mut huge = valid.clone();
    huge[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    for stored in [vec![0, 0, 0], wrong_size, surplus, bad_header, huge] {
        fs::write(
            &path,
            [
                header(),
                record(b"LAND", 0x200, plugin::COMPRESSED, &stored),
            ]
            .concat(),
        )
        .unwrap();
        for inspect in [false, true] {
            let limits = Limits {
                plugin: plugin::Limits {
                    inspect_checksum_mismatches: inspect,
                    ..plugin::Limits::default()
                },
                ..Limits::default()
            };
            assert!(compressed_records::inspect(&path, limits).is_err());
        }
    }
}

#[test]
fn compressed_row_and_decode_budgets_fail_without_partial_reports() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Base.esm");
    let stored = compressed(&[0; 1024]);
    fs::write(
        &path,
        [
            header(),
            record(b"LAND", 0x200, plugin::COMPRESSED, &stored),
        ]
        .concat(),
    )
    .unwrap();
    assert!(
        compressed_records::inspect(
            &path,
            Limits {
                max_rows: 0,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        compressed_records::inspect(
            &path,
            Limits {
                plugin: plugin::Limits {
                    max_decoded_bytes: 128,
                    ..plugin::Limits::default()
                },
                ..Limits::default()
            }
        )
        .is_err()
    );
}
