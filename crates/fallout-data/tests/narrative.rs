use fallout_data::{
    narrative::{self, SectionKind, Value},
    narrative_census, plugin,
};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], fields: &[Vec<u8>]) -> plugin::Record {
    plugin::Record {
        header: plugin::RecordHeader {
            kind: *kind,
            offset: 24,
            stored_size: 0,
            flags: 0,
            form_id: 1,
            revision: [0; 4],
            version: 15,
            trailing_bytes: [0; 2],
        },
        payload: fields.concat(),
        integrity_issue: None,
    }
}
fn script() -> Vec<Vec<u8>> {
    vec![
        field(b"SCHR", &[0; 20]),
        field(b"SCTX", b"original fixture\0"),
    ]
}
fn decode(record: &plugin::Record) -> narrative::Document<'_> {
    narrative::decode(record, "Fixture.esm", narrative::Limits::default()).unwrap()
}

#[test]
fn repeated_stage_keys_keep_distinct_entries_conditions_and_scripts() {
    let record = record(
        b"QUST",
        &[
            vec![
                field(b"DATA", &[1, 50, 7, 8]),
                field(b"CTDA", &[0; 20]),
                field(b"INDX", &10_i16.to_le_bytes()),
                field(b"QSDT", &[1]),
                field(b"CNAM", b"first\0"),
                field(b"CTDA", &[0; 20]),
            ],
            script(),
            vec![field(b"QSDT", &[2]), field(b"CNAM", b"second\0")],
            script(),
            vec![field(b"INDX", &10_i16.to_le_bytes()), field(b"QSDT", &[0])],
            script(),
            vec![
                field(b"QOBJ", &(-3_i32).to_le_bytes()),
                field(b"NNAM", b"objective\0"),
                field(b"QSTA", &[1, 2, 3, 4, 5, 6, 7, 8]),
                field(b"CTDA", &[0; 24]),
            ],
        ]
        .concat(),
    );
    let doc = decode(&record);
    assert!(doc.findings.is_empty());
    let stages: Vec<_> = doc
        .sections
        .iter()
        .enumerate()
        .filter(|(_, section)| section.kind == SectionKind::Stage)
        .collect();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].1.key, Some(10));
    assert_eq!(stages[1].1.key, Some(10));
    assert_ne!(stages[0].1.marker_offset, stages[1].1.marker_offset);
    let entries: Vec<_> = doc
        .sections
        .iter()
        .enumerate()
        .filter(|(_, section)| section.kind == SectionKind::LogEntry)
        .collect();
    assert_eq!(
        entries
            .iter()
            .map(|(_, entry)| entry.parent)
            .collect::<Vec<_>>(),
        vec![Some(stages[0].0), Some(stages[0].0), Some(stages[1].0)]
    );
    assert_eq!(
        doc.scripts
            .iter()
            .map(|script| script.owner)
            .collect::<Vec<_>>(),
        entries
            .iter()
            .map(|(index, _)| Some(*index))
            .collect::<Vec<_>>()
    );
    let owners: Vec<_> = doc
        .fields
        .iter()
        .filter(|field| field.kind == *b"CTDA")
        .map(|field| doc.sections[field.owner.unwrap()].kind)
        .collect();
    assert_eq!(
        owners,
        vec![
            SectionKind::Quest,
            SectionKind::LogEntry,
            SectionKind::Target
        ]
    );
    assert_eq!(
        doc.sections
            .iter()
            .find(|section| section.kind == SectionKind::Objective)
            .unwrap()
            .key,
        Some(-3)
    );
    match doc.fields[0].value {
        Value::QuestData(data) => {
            assert_eq!(data.padding, Some([7, 8]));
            assert_eq!(data.delay_bits, None);
        }
        _ => panic!("quest data"),
    }
    let text = doc
        .fields
        .iter()
        .find(|field| field.kind == *b"CNAM")
        .unwrap();
    match text.value {
        Value::Text(bytes) => assert!(std::ptr::eq(
            bytes.as_ptr(),
            record.payload[text.offset + 6..].as_ptr()
        )),
        _ => panic!("borrowed text"),
    }
}

#[test]
fn response_text_conditions_and_begin_end_scripts_have_separate_owners() {
    let record = record(
        b"INFO",
        &[
            vec![
                field(b"DATA", &[0, 2, 4]),
                field(b"QSTI", &9_u32.to_le_bytes()),
                field(b"TRDT", &[0; 20]),
                field(b"NAM1", b"first\0"),
                field(b"NAM2", b"notes\0"),
                field(b"TRDT", &[0; 24]),
                field(b"NAM1", b"second\0"),
                field(b"CTDA", &[0; 28]),
            ],
            script(),
            vec![field(b"NEXT", &[])],
            script(),
            vec![field(b"RNAM", b"prompt\0")],
        ]
        .concat(),
    );
    let doc = decode(&record);
    assert!(doc.findings.is_empty());
    let responses: Vec<_> = doc
        .sections
        .iter()
        .filter(|section| section.kind == SectionKind::Response)
        .collect();
    assert_eq!(responses.len(), 2);
    assert_eq!(
        doc.fields
            .iter()
            .find(|field| field.kind == *b"CTDA")
            .unwrap()
            .owner,
        Some(0)
    );
    assert_eq!(
        doc.sections[doc.scripts[0].owner.unwrap()].kind,
        SectionKind::BeginScript
    );
    assert_eq!(
        doc.sections[doc.scripts[1].owner.unwrap()].kind,
        SectionKind::EndScript
    );
    assert_eq!(doc.fields.last().unwrap().owner, Some(0));
    match doc.fields[0].value {
        Value::InfoData(data) => assert_eq!(data.flags2, None),
        _ => panic!("info data"),
    }
    let response = doc
        .fields
        .iter()
        .find(|field| field.kind == *b"TRDT")
        .unwrap();
    match response.value {
        Value::ResponseData(data) => {
            assert_eq!(data.use_emotion_animation, None);
            assert_eq!(data.animation_padding, None);
        }
        _ => panic!("response data"),
    }
}

#[test]
fn orphaned_shared_infos_are_not_attached_to_an_invented_quest() {
    let record = record(
        b"DIAL",
        &[
            field(b"EDID", b"topic\0"),
            field(b"INFC", &1_u32.to_le_bytes()),
            field(b"INFX", &(-1_i32).to_le_bytes()),
            field(b"QSTI", &2_u32.to_le_bytes()),
            field(b"INFC", &3_u32.to_le_bytes()),
            field(b"INFX", &7_i32.to_le_bytes()),
            field(b"FULL", b"title\0"),
            field(b"INFC", &4_u32.to_le_bytes()),
            field(b"ZZZZ", &[1, 2, 3]),
        ],
    );
    let doc = decode(&record);
    let connections: Vec<_> = doc
        .sections
        .iter()
        .filter(|section| section.kind == SectionKind::InfoConnection)
        .collect();
    assert_eq!(connections.len(), 3);
    assert_eq!(connections[0].parent, None);
    assert!(connections[1].parent.is_some());
    assert_eq!(connections[2].parent, None);
    assert_eq!(doc.findings.len(), 3);
    assert_eq!(doc.fields.last().unwrap().owner, None);
    assert_eq!(
        narrative::evidence_words(&doc.fields[2].value),
        (3, vec![u64::MAX])
    );
}

#[test]
fn incomplete_authored_markers_remain_diagnostic_and_limits_are_enforced() {
    let record = record(
        b"QUST",
        &[
            field(b"QSDT", &[0]),
            field(b"CTDA", &[0; 20]),
            field(b"QSTA", &[0; 8]),
            field(b"CTDA", &[0; 20]),
        ],
    );
    let doc = decode(&record);
    assert_eq!(doc.findings.len(), 2);
    assert_eq!(doc.sections[1].parent, None);
    assert_eq!(doc.sections[2].parent, None);
    assert_eq!(doc.fields[1].owner, Some(1));
    assert_eq!(doc.fields[3].owner, Some(2));
    for limits in [
        narrative::Limits {
            max_fields: 1,
            ..Default::default()
        },
        narrative::Limits {
            max_sections: 1,
            ..Default::default()
        },
        narrative::Limits {
            max_findings: 0,
            ..Default::default()
        },
    ] {
        assert!(narrative::decode(&record, "Fixture.esm", limits).is_err());
    }
}

#[test]
fn invalid_known_field_lengths_and_truncated_framing_fail_without_panics() {
    for (kind, field_kind, allowed) in [
        (b"QUST", b"DATA", vec![2, 4, 8]),
        (b"INFO", b"DATA", vec![3, 4]),
        (b"DIAL", b"DATA", vec![1, 2]),
        (b"INFO", b"TRDT", vec![20, 24]),
        (b"QUST", b"INDX", vec![2]),
        (b"QUST", b"QSTA", vec![8]),
        (b"INFO", b"QSTI", vec![4]),
        (b"INFO", b"NEXT", vec![0]),
    ] {
        for size in 0..=30 {
            let record = record(kind, &[field(field_kind, &vec![0; size])]);
            assert_eq!(
                narrative::decode(&record, "Fixture.esm", Default::default()).is_ok(),
                allowed.contains(&size),
                "{field_kind:?}/{size}"
            );
        }
    }
    let mut record = record(b"QUST", &[field(b"EDID", b"fixture\0")]);
    for length in 1..record.payload.len() {
        let original = record.payload.clone();
        record.payload.truncate(length);
        assert!(narrative::decode(&record, "Fixture.esm", Default::default()).is_err());
        record.payload = original;
    }
}

#[test]
fn evidence_changes_when_owner_bits_or_unknown_bytes_change() {
    let first = record(
        b"INFO",
        &[field(b"DATA", &[0, 1, 2, 3]), field(b"ZZZZ", &[4, 5])],
    );
    let second = record(
        b"INFO",
        &[field(b"DATA", &[0, 1, 2, 4]), field(b"ZZZZ", &[4, 5])],
    );
    let third = record(
        b"INFO",
        &[field(b"DATA", &[0, 1, 2, 3]), field(b"ZZZZ", &[4, 6])],
    );
    let mut document = decode(&first);
    let digest = narrative_census::fields_digest(&document);
    assert_ne!(digest, narrative_census::fields_digest(&decode(&second)));
    assert_ne!(digest, narrative_census::fields_digest(&decode(&third)));
    document.fields[0].owner = None;
    assert_ne!(digest, narrative_census::fields_digest(&document));
}
