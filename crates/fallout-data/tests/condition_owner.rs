use fallout_data::{
    condition_operands::{self, OwnerLimits, OwnerStatus, RecordLimits, Signatures},
    identity::{FormKey, ProfileId},
    narrative::SectionKind,
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(raw.len() as u16).to_le_bytes(), raw].concat()
}
fn record(kind: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn condition(size: usize, or: bool) -> Vec<u8> {
    let mut raw = vec![0; 28];
    raw[0] = u8::from(or);
    raw[4..8].copy_from_slice(&0x3f800000_u32.to_le_bytes());
    raw[8..10].copy_from_slice(&65535_u16.to_le_bytes());
    raw[20..24].copy_from_slice(&7_u32.to_le_bytes());
    raw.truncate(size);
    field(b"CTDA", &raw)
}
fn key() -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: 0x100,
    }
}
fn fixture(path: &Path, kind: &[u8; 4], body: &[u8]) -> RecordStore {
    let header = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    fs::write(
        path.join("FalloutNV.esm"),
        [record(b"TES4", 0, &header), record(kind, 0x100, body)].concat(),
    )
    .unwrap();
    RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
        .unwrap()
}
fn quest() -> Vec<u8> {
    [
        condition(20, true),
        condition(28, false),
        field(b"INDX", &(-42_i16).to_le_bytes()),
        condition(24, true),
        field(b"QSDT", &[0]),
        condition(28, true),
        field(b"QSDT", &[0]),
        condition(20, false),
        field(b"INDX", &(-42_i16).to_le_bytes()),
        condition(20, true),
        field(b"QSDT", &[0]),
        condition(24, false),
        field(b"QOBJ", &42_i32.to_le_bytes()),
        condition(20, true),
        field(b"QSTA", &[0; 8]),
        condition(28, true),
        field(b"QSTA", &[0; 8]),
        condition(20, false),
    ]
    .concat()
}

#[test]
fn quest_source_containers_keep_repeated_keys_distinct_and_orphans_unassigned() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = fixture(directory.path(), b"QUST", &quest());
    let location = source.winner(&key()).unwrap();
    let prepared = condition_operands::prepare_record_with_owners(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits::default(),
        OwnerLimits::default(),
    )
    .unwrap();
    let owners = prepared.ownership();
    assert_eq!(owners.status(), OwnerStatus::MappedNarrativeSource);
    assert_eq!(
        owners
            .sites()
            .iter()
            .map(|site| site.owner_section)
            .collect::<Vec<_>>(),
        [
            Some(0),
            Some(0),
            None,
            Some(2),
            Some(3),
            None,
            Some(5),
            None,
            Some(7),
            Some(8)
        ]
    );
    assert_eq!(
        owners.sections().iter().map(|s| s.kind).collect::<Vec<_>>(),
        [
            SectionKind::Quest,
            SectionKind::Stage,
            SectionKind::LogEntry,
            SectionKind::LogEntry,
            SectionKind::Stage,
            SectionKind::LogEntry,
            SectionKind::Objective,
            SectionKind::Target,
            SectionKind::Target
        ]
    );
    assert_eq!(owners.sections()[1].key, Some(-42));
    assert_eq!(owners.sections()[4].key, Some(-42));
    assert_ne!(
        owners.sections()[1].marker_offset,
        owners.sections()[4].marker_offset
    );
    assert_eq!(
        owners
            .source_lists()
            .iter()
            .map(|g| (g.owner_section, g.site_indices.as_slice()))
            .collect::<Vec<_>>(),
        [
            (0, &[0, 1][..]),
            (2, &[3][..]),
            (3, &[4][..]),
            (5, &[6][..]),
            (7, &[8][..]),
            (8, &[9][..])
        ]
    );
    assert_eq!(owners.findings().len(), 3);
    for (site, owner) in prepared.conditions().sites().iter().zip(owners.sites()) {
        assert_eq!(site.field_decoded_offset(), owner.field_decoded_offset);
    }
    assert_eq!(
        prepared
            .conditions()
            .sites()
            .iter()
            .map(|s| s.condition().or_flag())
            .collect::<Vec<_>>(),
        [
            true, false, true, true, false, true, false, true, true, false
        ]
    );
    let report = serde_json::to_value(owners).unwrap();
    assert_eq!(report["evaluation_ready"], false);
    assert_eq!(report["group_evaluation_verified"], false);
    assert_eq!(report["default_subjects_applied"], false);
    assert!(owners.narrative_fields_sha256().is_some());
}

#[test]
fn info_conditions_keep_root_owner_after_responses_and_next_marker() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        field(b"TRDT", &[0; 20]),
        field(b"NAM1", b"Authored\0"),
        condition(20, true),
        field(b"TRDT", &[0; 24]),
        condition(28, false),
        field(b"NEXT", &[]),
        condition(24, true),
    ]
    .concat();
    let mut source = fixture(directory.path(), b"INFO", &body);
    let location = source.winner(&key()).unwrap();
    let prepared = condition_operands::prepare_record_with_owners(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits::default(),
        OwnerLimits::default(),
    )
    .unwrap();
    assert!(
        prepared
            .ownership()
            .sites()
            .iter()
            .all(|s| s.owner_section == Some(0))
    );
    assert_eq!(
        prepared
            .ownership()
            .sections()
            .iter()
            .map(|s| s.kind)
            .collect::<Vec<_>>(),
        [
            SectionKind::DialogueInfo,
            SectionKind::Response,
            SectionKind::Response
        ]
    );
    assert_eq!(
        prepared.ownership().source_lists()[0].site_indices,
        [0, 1, 2]
    );
    assert!(prepared.ownership().findings().is_empty());
}

#[test]
fn unmapped_record_kinds_do_not_acquire_a_fabricated_root_list() {
    for kind in [b"PACK", b"SPEL", b"DIAL"] {
        let directory = tempfile::tempdir().unwrap();
        let mut source = fixture(
            directory.path(),
            kind,
            &[condition(20, true), condition(28, false)].concat(),
        );
        let location = source.winner(&key()).unwrap();
        let prepared = condition_operands::prepare_record_with_owners(
            &mut source,
            location,
            &Signatures::new(),
            RecordLimits::default(),
            OwnerLimits::default(),
        )
        .unwrap();
        let owners = prepared.ownership();
        assert_eq!(owners.status(), OwnerStatus::UnmappedRecordKind);
        assert!(owners.sections().is_empty() && owners.source_lists().is_empty());
        assert_eq!(owners.sites().len(), 2);
        assert!(owners.sites().iter().all(|s| s.owner_section.is_none()));
        assert!(owners.narrative_fields_sha256().is_none());
    }
}

#[test]
fn owner_metadata_exact_byte_limit_admits_and_one_less_fails_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = fixture(directory.path(), b"QUST", &quest());
    let location = source.winner(&key()).unwrap();
    let prepared = condition_operands::prepare_record_with_owners(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits::default(),
        OwnerLimits::default(),
    )
    .unwrap();
    let exact = serde_json::to_vec(prepared.ownership()).unwrap().len();
    assert_eq!(prepared.ownership().retained_bytes(), exact);
    for maximum in [0, exact - 1] {
        assert!(
            condition_operands::prepare_record_with_owners(
                &mut source,
                location,
                &Signatures::new(),
                RecordLimits::default(),
                OwnerLimits {
                    maximum_retained_bytes: maximum,
                    ..OwnerLimits::default()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("owner metadata byte budget")
        );
    }
    assert!(
        condition_operands::prepare_record_with_owners(
            &mut source,
            location,
            &Signatures::new(),
            RecordLimits::default(),
            OwnerLimits {
                maximum_retained_bytes: exact,
                ..OwnerLimits::default()
            }
        )
        .is_ok()
    );
}

#[test]
fn section_and_finding_limits_preserve_unknown_ownership_instead_of_defaulting() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = fixture(directory.path(), b"QUST", &quest());
    let location = source.winner(&key()).unwrap();
    for (limits, reason) in [
        (
            OwnerLimits {
                maximum_sections: 1,
                ..OwnerLimits::default()
            },
            "section budget",
        ),
        (
            OwnerLimits {
                maximum_findings: 0,
                ..OwnerLimits::default()
            },
            "finding budget",
        ),
    ] {
        assert!(
            condition_operands::prepare_record_with_owners(
                &mut source,
                location,
                &Signatures::new(),
                RecordLimits::default(),
                limits
            )
            .unwrap_err()
            .to_string()
            .contains(reason)
        );
    }
}

#[test]
fn optional_owner_admission_rejects_malformed_owner_fields_but_preserves_legacy_rows() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = fixture(
        directory.path(),
        b"QUST",
        &[field(b"QSTA", &[0]), condition(20, true)].concat(),
    );
    let location = source.winner(&key()).unwrap();
    assert!(
        condition_operands::prepare_record(
            &mut source,
            location,
            &Signatures::new(),
            RecordLimits::default()
        )
        .is_ok()
    );
    assert!(
        condition_operands::prepare_record_with_owners(
            &mut source,
            location,
            &Signatures::new(),
            RecordLimits::default(),
            OwnerLimits::default()
        )
        .unwrap_err()
        .to_string()
        .contains("invalid field length")
    );
}

#[test]
fn optional_source_ownership_preserves_default_identity_and_binding_projection() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = fixture(directory.path(), b"QUST", &quest());
    let location = source.winner(&key()).unwrap();
    let plain = condition_operands::prepare_record(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits::default(),
    )
    .unwrap();
    let owned = condition_operands::prepare_record_with_owners(
        &mut source,
        location,
        &Signatures::new(),
        RecordLimits::default(),
        OwnerLimits::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&plain).unwrap(),
        serde_json::to_vec(owned.conditions()).unwrap()
    );
}
