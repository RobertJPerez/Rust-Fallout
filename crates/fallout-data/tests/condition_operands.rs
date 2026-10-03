use fallout_data::{
    condition,
    condition_operands::{
        self, Domain, FormStatus, Parameter, Signature, SignatureStatus, Subject, Value,
    },
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

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
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for name in masters {
        body.extend(field(b"MAST", &[name.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn fixture(path: &Path) -> RecordStore {
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"QUST", 0x100, 0, &[]),
            record(b"MISC", 0x200, 0, &[]),
            record(b"MISC", 0x201, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        path.join("Patch.esp"),
        [
            header(&["FalloutNV.esm"]),
            record(
                b"MISC",
                0x200,
                plugin::DELETED,
                b"deleted body stays deferred",
            ),
            record(b"QUST", 0x01000100, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    RecordStore::open_nv_headers(
        path,
        &["FalloutNV.esm".into(), "Patch.esp".into()],
        plugin::Limits::default(),
    )
    .unwrap()
}
fn key(plugin: &str, id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: plugin.into(),
        local_id: id,
    }
}
fn words(function: u16, parameters: [u32; 2], run: u32, reference: u32) -> [u8; 28] {
    let mut bytes = [0; 28];
    bytes[8..10].copy_from_slice(&function.to_le_bytes());
    bytes[12..16].copy_from_slice(&parameters[0].to_le_bytes());
    bytes[16..20].copy_from_slice(&parameters[1].to_le_bytes());
    bytes[20..24].copy_from_slice(&run.to_le_bytes());
    bytes[24..28].copy_from_slice(&reference.to_le_bytes());
    bytes
}
fn signature(types: &[u32]) -> Signature {
    Signature {
        parameters: types
            .iter()
            .map(|id| Parameter {
                type_id: *id,
                optional_word: 0,
            })
            .collect(),
    }
}

#[test]
fn condition_words_keep_full_width_float_bits_signed_values_and_unused_data() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let source = store.winner(&key("falloutnv.esm", 0x100)).unwrap();
    let bytes = words(1, [0xffc01234, 0xf3e112ab], 0, 0);
    let value = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes).unwrap(),
        Some(&signature(&[2])),
    )
    .unwrap();
    assert_eq!(
        value.operands[0].value,
        Value::FloatBits {
            raw_word: 0xffc01234
        }
    );
    assert_eq!(
        value.operands[1].value,
        Value::Unused {
            raw_word: 0xf3e112ab
        }
    );
    assert!(value.operands.iter().all(|p| p.form_dependency.is_none()));
    let value = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes).unwrap(),
        Some(&signature(&[1, 22])),
    )
    .unwrap();
    assert_eq!(
        value.operands[0].value,
        Value::SignedInteger {
            raw_word: 0xffc01234,
            value: 0xffc01234_u32 as i32
        }
    );
    assert_eq!(
        value.operands[1].value,
        Value::VariableIndex {
            raw_word: 0xf3e112ab,
            signed_index: 0xf3e112ab_u32 as i32
        }
    );
    assert!(!value.live_values_resolved && !value.evaluation_ready);
}

#[test]
fn vats_selector_changes_the_second_word_type_and_preserves_unused_words() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let source = store.winner(&key("falloutnv.esm", 0x100)).unwrap();
    for selector in 0..=18 {
        let bytes = words(408, [selector, 0xffff_ffd6], 0, 0);
        let bound = condition_operands::bind(
            &store,
            source,
            &condition::decode(&bytes).unwrap(),
            Some(&signature(&[1, 1])),
        )
        .unwrap();
        assert_eq!(
            bound.operands[0].value,
            Value::UnsignedDomain {
                raw_word: selector,
                domain: Domain::VatsFunction
            }
        );
        match selector {
            0..=3 | 9 | 10 => {
                assert!(matches!(bound.operands[1].value, Value::FormId { .. }));
                assert!(bound.operands[1].form_dependency.is_some());
            }
            5 => assert_eq!(
                bound.operands[1].value,
                Value::SignedDomain {
                    raw_word: 0xffff_ffd6,
                    value: -42,
                    domain: Domain::ActorValue
                }
            ),
            6 | 15 => assert!(matches!(
                bound.operands[1].value,
                Value::UnsignedDomain { .. }
            )),
            18 => {
                assert!(matches!(bound.operands[1].value, Value::Unknown { .. }));
                assert_eq!(bound.signature_status, SignatureStatus::SchemaDisagreement);
            }
            _ => {
                assert_eq!(
                    bound.operands[1].value,
                    Value::Unused {
                        raw_word: 0xffff_ffd6
                    }
                );
                assert!(bound.operands[1].form_dependency.is_none());
            }
        }
    }
}

#[test]
fn subject_words_are_distinct_from_animation_groups_and_legacy_editor_migrations() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let source = store.winner(&key("falloutnv.esm", 0x100)).unwrap();
    let mut bytes = words(106, [0, 0], 2, 0x201);
    bytes[0] = 2;
    let bound = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes).unwrap(),
        Some(&signature(&[])),
    )
    .unwrap();
    assert_eq!(bound.subject, Subject::AnimationGroup { raw_word: Some(2) });
    assert!(bound.subject_reference.is_none());
    bytes[8..10].copy_from_slice(&1_u16.to_le_bytes());
    let bound = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes[..24]).unwrap(),
        Some(&signature(&[])),
    )
    .unwrap();
    assert_eq!(bound.subject, Subject::Reference { raw_word: None });
    assert!(bound.subject_reference.is_none());
    let bound = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes[..20]).unwrap(),
        Some(&signature(&[])),
    )
    .unwrap();
    assert_eq!(bound.subject, Subject::Absent);
    assert!(bound.legacy_target_flag_present);
    for (run, expected) in [
        (0_u32, Subject::Subject),
        (1, Subject::Target),
        (3, Subject::CombatTarget),
        (4, Subject::LinkedReference),
        (7, Subject::Unknown { raw_word: 7 }),
    ] {
        bytes[20..24].copy_from_slice(&run.to_le_bytes());
        let bound = condition_operands::bind(
            &store,
            source,
            &condition::decode(&bytes).unwrap(),
            Some(&signature(&[])),
        )
        .unwrap();
        assert_eq!(bound.subject, expected);
        assert!(bound.subject_reference.is_none());
    }
}

#[test]
fn static_dependencies_resolve_in_the_source_namespace_and_keep_tombstones() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let source = store.winner(&key("patch.esp", 0x100)).unwrap();
    let mut bytes = words(1, [0x200, 0x201], 2, 0x14);
    bytes[0] = 4;
    bytes[4..8].copy_from_slice(&0x201_u32.to_le_bytes());
    let bound = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes).unwrap(),
        Some(&signature(&[4, 4])),
    )
    .unwrap();
    let deleted = bound.operands[0].form_dependency.as_ref().unwrap();
    assert_eq!(deleted.status, FormStatus::Deleted);
    assert_eq!(deleted.key, Some(key("falloutnv.esm", 0x200)));
    assert_eq!(deleted.target.as_ref().unwrap().source_name, "Patch.esp");
    assert_eq!(
        bound.operands[1].form_dependency.as_ref().unwrap().status,
        FormStatus::Defined
    );
    assert_eq!(
        bound.comparison_global.as_ref().unwrap().key,
        Some(key("falloutnv.esm", 0x201))
    );
    assert_eq!(
        bound.subject_reference.as_ref().unwrap().status,
        FormStatus::RuntimeDependency
    );
    assert_eq!(
        condition_operands::form_dependency(&store, source, 0)
            .unwrap()
            .status,
        FormStatus::Null
    );
    assert_eq!(
        condition_operands::form_dependency(&store, source, 0x203)
            .unwrap()
            .status,
        FormStatus::Missing
    );
    assert_eq!(
        condition_operands::form_dependency(&store, source, 0x02000100)
            .unwrap()
            .key,
        Some(key("patch.esp", 0x100))
    );
    assert_eq!(
        condition_operands::form_dependency(&store, source, 0x02000014)
            .unwrap()
            .status,
        FormStatus::Missing
    );
}

#[test]
fn unknown_descriptors_signatures_and_optional_words_stay_unresolved() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let source = store.winner(&key("falloutnv.esm", 0x100)).unwrap();
    let bytes = words(65535, [0x200, 0x201], 0, 0);
    let condition = condition::decode(&bytes).unwrap();
    for (signature, expected) in [
        (None, SignatureStatus::MissingDescriptor),
        (
            Some(signature(&[4, 4, 4])),
            SignatureStatus::AdditionalParameters,
        ),
        (
            Some(Signature {
                parameters: vec![Parameter {
                    type_id: 4,
                    optional_word: 2,
                }],
            }),
            SignatureStatus::UnverifiedOptionalWord,
        ),
        (
            Some(signature(&[0x100])),
            SignatureStatus::SchemaDisagreement,
        ),
    ] {
        let bound =
            condition_operands::bind(&store, source, &condition, signature.as_ref()).unwrap();
        assert_eq!(bound.signature_status, expected);
        assert!(bound.operands[0].form_dependency.is_none());
        assert_eq!(bound.operands[0].value, Value::Unknown { raw_word: 0x200 });
    }
    let bytes = words(427, [0x200, 0], 0, 0);
    let voice = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes).unwrap(),
        Some(&signature(&[46])),
    )
    .unwrap();
    assert_eq!(
        voice.operands[0].value,
        Value::FormId {
            raw_word: 0x200,
            domain: Domain::VoiceType
        }
    );
    let wrong = condition_operands::bind(
        &store,
        source,
        &condition::decode(&bytes).unwrap(),
        Some(&signature(&[4])),
    )
    .unwrap();
    assert_eq!(wrong.signature_status, SignatureStatus::SchemaDisagreement);
    assert!(wrong.operands[0].form_dependency.is_none());
}
