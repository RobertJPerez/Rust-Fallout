use fallout_data::{
    condition_operands::{
        self, FormStatus, Parameter, RecordLimits, Signature, SignatureStatus, Signatures, Subject,
    },
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn header(master: bool) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    if master {
        body.extend(field(b"MAST", b"FalloutNV.esm\0"));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn words(size: usize, function: u16, flags: u8, run: u32) -> Vec<u8> {
    let mut bytes = [0; 28];
    bytes[0] = flags;
    bytes[4..8].copy_from_slice(&0x7fc01234_u32.to_le_bytes());
    bytes[8..10].copy_from_slice(&function.to_le_bytes());
    bytes[12..16].copy_from_slice(&0x200_u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&0xff000014_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&run.to_le_bytes());
    bytes[24..28].copy_from_slice(&0x14_u32.to_le_bytes());
    bytes[..size].to_vec()
}
fn store(path: &Path, body: &[u8], flags: u32, patch: bool, extra: bool) -> RecordStore {
    let mut base = [
        header(false),
        record(b"PACK", 0x100, flags, body),
        record(b"MISC", 0x200, 0, &[]),
    ]
    .concat();
    if extra {
        base.extend(record(b"MISC", 0x300, 0, &[]));
    }
    fs::write(path.join("FalloutNV.esm"), base).unwrap();
    let mut order = vec!["FalloutNV.esm".into()];
    if patch {
        fs::write(
            path.join("Patch.esp"),
            [
                header(true),
                record(b"PACK", 0x100, 0, body),
                record(b"MISC", 0x200, plugin::DELETED, b"not decoded"),
            ]
            .concat(),
        )
        .unwrap();
        order.push("Patch.esp".into());
    }
    RecordStore::open_nv_headers(path, &order, plugin::Limits::default()).unwrap()
}

#[test]
fn immutable_sites_preserve_physical_order_raw_layouts_and_existing_subjects() {
    let directory = tempfile::tempdir().unwrap();
    let raws = [
        words(20, 65535, 3, 0),
        words(24, 65535, 0xc0, 7),
        words(28, 65535, 1, 2),
    ];
    let body = [
        field(b"EDID", b"Pack\0"),
        field(b"CTDA", &raws[0]),
        field(b"CTDA", &raws[1]),
        field(b"UNKN", &[0xff]),
        field(b"CTDA", &raws[2]),
    ]
    .concat();
    let mut source = store(directory.path(), &body, 0, false, false);
    let location = source.winner(&key(0x100)).unwrap();
    let prepared = condition_operands::prepare_record(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits::default(),
    )
    .unwrap();
    assert_eq!(prepared.fields(), 5);
    assert_eq!(prepared.identity().key, key(0x100));
    assert_eq!(prepared.identity().record_kind, "PACK");
    assert_eq!(prepared.identity().decoded_bytes, body.len());
    assert_eq!(
        prepared.identity().decoded_sha256,
        format!("{:x}", Sha256::digest(&body))
    );
    assert_eq!(
        prepared.identity().source_sha256,
        format!(
            "{:x}",
            Sha256::digest(fs::read(directory.path().join("FalloutNV.esm")).unwrap())
        )
    );
    let sites = prepared.sites();
    assert_eq!(
        sites
            .iter()
            .map(|s| s.field_decoded_offset())
            .collect::<Vec<_>>(),
        [11, 37, 74]
    );
    assert_eq!(
        sites
            .iter()
            .map(|s| s.condition().or_flag())
            .collect::<Vec<_>>(),
        [true, false, true]
    );
    assert_eq!(sites[0].binding().subject, Subject::Absent);
    assert_eq!(sites[1].binding().subject, Subject::Unknown { raw_word: 7 });
    assert_eq!(
        sites[2].binding().subject,
        Subject::Reference {
            raw_word: Some(0x14)
        }
    );
    for (site, raw) in sites.iter().zip(&raws) {
        assert_eq!(site.raw_bytes(), raw);
        assert_eq!(
            site.binding().signature_status,
            SignatureStatus::MissingDescriptor
        );
        assert!(!site.binding().live_values_resolved && !site.binding().evaluation_ready);
    }
    let row = serde_json::to_value(sites[2].legacy_row()).unwrap();
    assert_eq!(sites[2].preceding_field_kind(), Some("UNKN"));
    assert_eq!(sites[2].preceding_field_decoded_offset(), Some(67));
    assert_eq!(row["preceding_field_kind"], "UNKN");
    assert_eq!(row["preceding_field_decoded_offset"], 67);
    assert_eq!(row["reference_word"], 0x14);
    assert_eq!(
        sites[1].source_findings(),
        [
            "unknown_comparison_operator",
            "nonfinite_comparison",
            "unknown_subject_selector"
        ]
    );
}

#[test]
fn exact_winner_and_deleted_source_are_rejected_before_deferred_body_read() {
    let base_dir = tempfile::tempdir().unwrap();
    let base = store(base_dir.path(), &[], 0, false, false);
    let old_location = base.winner(&key(0x100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut source = store(directory.path(), &field(b"CTDA", &[0; 22]), 0, true, false);
    let error = condition_operands::prepare_record(
        &mut source,
        old_location,
        &Signatures::new(),
        RecordLimits::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("exact winning record"));
    let deleted = source.winner(&key(0x200)).unwrap();
    assert!(
        condition_operands::prepare_record(
            &mut source,
            deleted,
            &Signatures::new(),
            RecordLimits::default()
        )
        .unwrap_err()
        .to_string()
        .contains("deleted")
    );
}

#[test]
fn parameter_dependencies_use_the_winning_sources_namespace_and_keep_tombstones() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = store(
        directory.path(),
        &field(b"CTDA", &words(28, 1, 0, 2)),
        0,
        true,
        false,
    );
    let location = source.winner(&key(0x100)).unwrap();
    let signatures = [(
        1,
        Signature {
            parameters: vec![Parameter {
                type_id: 4,
                optional_word: 0,
            }],
        },
    )]
    .into_iter()
    .collect();
    let prepared = condition_operands::prepare_record(
        &mut source,
        location,
        &signatures,
        RecordLimits::default(),
    )
    .unwrap();
    assert_eq!(prepared.identity().source_name, "Patch.esp");
    let dependency = prepared.sites()[0].binding().operands[0]
        .form_dependency
        .as_ref()
        .unwrap();
    assert_eq!(dependency.key, Some(key(0x200)));
    assert_eq!(dependency.status, FormStatus::Deleted);
    assert_eq!(
        prepared.sites()[0]
            .binding()
            .subject_reference
            .as_ref()
            .unwrap()
            .status,
        FormStatus::RuntimeDependency
    );
}

#[test]
fn retained_budget_admits_exact_bytes_and_rejects_one_less_without_partial_result() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = store(
        directory.path(),
        &field(b"CTDA", &words(28, 65535, 0, 7)),
        0,
        false,
        false,
    );
    let location = source.winner(&key(0x100)).unwrap();
    let prepared = condition_operands::prepare_record(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits::default(),
    )
    .unwrap();
    let bytes = prepared.retained_bytes();
    assert_eq!(
        bytes,
        28 + serde_json::to_vec(&prepared.sites()[0].legacy_row())
            .unwrap()
            .len()
    );
    for budget in [0, 27, bytes - 1] {
        assert!(
            condition_operands::prepare_record(
                &mut source,
                location,
                &Signatures::new(),
                RecordLimits {
                    maximum_retained_bytes: budget,
                    ..RecordLimits::default()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("retained-site byte budget")
        );
    }
    let exact = condition_operands::prepare_record(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits {
            maximum_retained_bytes: bytes,
            ..RecordLimits::default()
        },
    )
    .unwrap();
    assert_eq!(exact.retained_bytes(), bytes);
}

#[test]
fn field_condition_and_stored_byte_bounds_are_enforced_at_admission() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        field(b"CTDA", &words(20, 65535, 0, 0)),
        field(b"CTDA", &words(24, 65535, 0, 0)),
    ]
    .concat();
    let mut source = store(directory.path(), &body, 0, false, false);
    let location = source.winner(&key(0x100)).unwrap();
    for (limits, expected) in [
        (
            RecordLimits {
                maximum_fields: 1,
                ..RecordLimits::default()
            },
            "field budget",
        ),
        (
            RecordLimits {
                maximum_conditions: 1,
                ..RecordLimits::default()
            },
            "row budget",
        ),
        (
            RecordLimits {
                maximum_decoded_bytes: body.len() - 1,
                ..RecordLimits::default()
            },
            "size budget",
        ),
    ] {
        assert!(
            condition_operands::prepare_record(&mut source, location, &Signatures::new(), limits)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
    }
    assert!(
        condition_operands::prepare_record(
            &mut source,
            location,
            &Signatures::new(),
            RecordLimits {
                maximum_fields: 2,
                maximum_conditions: 2,
                maximum_decoded_bytes: body.len(),
                ..RecordLimits::default()
            }
        )
        .is_ok()
    );
}

#[test]
fn compressed_claim_bound_and_checksum_are_not_relaxed_by_preparation() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        field(b"JUNK", &[0; 1024]),
        field(b"CTDA", &words(28, 65535, 0, 0)),
    ]
    .concat();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&body).unwrap();
    let mut compressed = encoder.finish().unwrap();
    let stored = [&(body.len() as u32).to_le_bytes()[..], &compressed].concat();
    let mut source = store(directory.path(), &stored, plugin::COMPRESSED, false, false);
    let location = source.winner(&key(0x100)).unwrap();
    let error = condition_operands::prepare_record(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits {
            maximum_decoded_bytes: body.len() - 1,
            ..RecordLimits::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("budget"));
    assert!(
        condition_operands::prepare_record(
            &mut source,
            location,
            &Signatures::new(),
            RecordLimits {
                maximum_decoded_bytes: body.len(),
                ..RecordLimits::default()
            }
        )
        .is_ok()
    );
    drop(source);
    *compressed.last_mut().unwrap() ^= 1;
    let stored = [&(body.len() as u32).to_le_bytes()[..], &compressed].concat();
    let mut corrupt = store(directory.path(), &stored, plugin::COMPRESSED, false, false);
    let location = corrupt.winner(&key(0x100)).unwrap();
    assert!(
        condition_operands::prepare_record(
            &mut corrupt,
            location,
            &Signatures::new(),
            RecordLimits::default()
        )
        .is_err()
    );
    drop(corrupt);
    let mut forensic = RecordStore::open_nv(
        directory.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits {
            inspect_checksum_mismatches: true,
            ..plugin::Limits::default()
        },
    )
    .unwrap();
    let location = forensic.winner(&key(0x100)).unwrap();
    assert!(forensic.read(location).unwrap().integrity_issue.is_some());
    assert!(
        condition_operands::prepare_record(
            &mut forensic,
            location,
            &Signatures::new(),
            RecordLimits::default()
        )
        .unwrap_err()
        .to_string()
        .contains("untrusted checksum recovery")
    );
}

#[test]
fn complete_ordered_source_receipts_change_even_when_record_bytes_do_not() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let body = field(b"CTDA", &words(20, 65535, 0, 0));
    let mut first = store(a.path(), &body, 0, false, false);
    let mut second = store(b.path(), &body, 0, false, true);
    let location = first.winner(&key(0x100)).unwrap();
    let other = second.winner(&key(0x100)).unwrap();
    let one = condition_operands::prepare_record(
        &mut first,
        location,
        &Signatures::new(),
        RecordLimits::default(),
    )
    .unwrap();
    let two = condition_operands::prepare_record(
        &mut second,
        other,
        &Signatures::new(),
        RecordLimits::default(),
    )
    .unwrap();
    assert_eq!(one.identity().decoded_sha256, two.identity().decoded_sha256);
    assert_ne!(
        one.identity().source_cohort_sha256,
        two.identity().source_cohort_sha256
    );
    assert_ne!(one.identity().source_sha256, two.identity().source_sha256);
}

#[test]
fn malformed_late_ctda_and_field_framing_return_no_prepared_record() {
    for suffix in [field(b"CTDA", &[0; 22]), vec![0; 5]] {
        let directory = tempfile::tempdir().unwrap();
        let body = [field(b"CTDA", &words(20, 65535, 0, 0)), suffix].concat();
        let mut source = store(directory.path(), &body, 0, false, false);
        let location = source.winner(&key(0x100)).unwrap();
        assert!(
            condition_operands::prepare_record(
                &mut source,
                location,
                &Signatures::new(),
                RecordLimits::default()
            )
            .is_err()
        );
    }
}
