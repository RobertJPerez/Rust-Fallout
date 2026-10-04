//! Independent physical two-INFO sources for all-or-none retained conversation batches.
use fallout_data::{
    condition_operands::Signatures,
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Limits as ScriptLimits, OwnerKind},
    plugin,
    store::RecordStore,
    world::conversation::{BatchLimits, ConversationBatch, DialogueSources, Limits, Request},
};
use std::{fs, io::Write};
fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(tag: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15_u16.to_le_bytes(),
        &[0; 2],
        data,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut data = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for name in masters {
        data.extend(field(b"MAST", &[name.as_bytes(), &[0]].concat()));
        data.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &data)
}
fn group(topic: u32, data: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(data.len() as u32 + 24).to_le_bytes(),
        &topic.to_le_bytes(),
        &7_i32.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}
fn condition(id: u16, reference: u32) -> [u8; 28] {
    let mut raw = [0; 28];
    raw[..4].copy_from_slice(&[0x21, 13, 17, 19]);
    raw[4..8].copy_from_slice(&0x80000000_u32.to_le_bytes());
    raw[8..10].copy_from_slice(&id.to_le_bytes());
    raw[10..12].copy_from_slice(&[23, 29]);
    raw[12..16].copy_from_slice(&0x11112222_u32.to_le_bytes());
    raw[16..20].copy_from_slice(&0x33334444_u32.to_le_bytes());
    raw[20..24].copy_from_slice(&0xfffffff0_u32.to_le_bytes());
    raw[24..].copy_from_slice(&reference.to_le_bytes());
    raw
}
fn script() -> Vec<u8> {
    let mut data = [0; 20];
    data[..4].copy_from_slice(&[61, 67, 71, 73]);
    data[8..12].copy_from_slice(&4_u32.to_le_bytes());
    [field(b"SCHR", &data), field(b"SCDA", &[0x1d, 0, 0, 0])].concat()
}
fn info(second: bool, mode: &str) -> Vec<u8> {
    let mut trdt = [0; 24];
    trdt[..4].copy_from_slice(&7_u32.to_le_bytes());
    trdt[4..8].copy_from_slice(&(-3_i32).to_le_bytes());
    trdt[8..12].copy_from_slice(&[31, 37, 41, 43]);
    trdt[12] = if second { 2 } else { 9 };
    trdt[13..16].copy_from_slice(&[47, 53, 59]);
    let text = if second {
        &[254, 129, 66, 0]
    } else {
        &[255, 128, 65, 0]
    };
    let mut data = [
        field(b"DATA", &[0, 7, 0, 0]),
        field(b"TPIC", &0x100_u32.to_le_bytes()),
        field(
            b"CTDA",
            &condition(if second { 65534 } else { 65535 }, 0x400),
        ),
        field(b"TRDT", &trdt),
    ]
    .concat();
    if mode == "xxxx" {
        data.extend(field(b"XXXX", &4_u32.to_le_bytes()));
        data.extend([b"NAM1".as_slice(), &0_u16.to_le_bytes(), text].concat());
    } else {
        data.extend(field(b"NAM1", text));
    }
    data.extend(field(b"NAM1", b"repeat\0"));
    data.extend(script());
    if mode == "ambiguous" {
        data.extend(script());
    }
    data.extend(field(b"NEXT", &[]));
    data.extend(script());
    if mode == "truncated" {
        data.extend(b"NAM1\x08\0x");
    }
    data
}
fn source(mode: &str) -> Vec<u8> {
    let topic = [
        field(b"EDID", b"batch\0"),
        field(b"FULL", &[255, 128, 84, 0]),
        field(b"DATA", &[0]),
        field(b"QSTI", &0x500_u32.to_le_bytes()),
    ]
    .concat();
    let first = record(b"INFO", 0x300, 0, &info(false, ""));
    let payload = info(true, mode);
    let second = if mode == "compressed" || mode == "tainted" {
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(&payload).unwrap();
        let mut encoded = compressor.finish().unwrap();
        if mode == "tainted" {
            *encoded.last_mut().unwrap() ^= 1;
        }
        record(
            b"INFO",
            0x301,
            plugin::COMPRESSED,
            &[(payload.len() as u32).to_le_bytes().as_slice(), &encoded].concat(),
        )
    } else {
        record(b"INFO", 0x301, 0, &payload)
    };
    [
        header(&[]),
        record(b"DIAL", 0x100, 0, &topic),
        record(b"NPC_", 0x400, 0, &[]),
        record(b"QUST", 0x500, 0, &[]),
        record(b"DIAL", 0x200, 0, &field(b"DATA", &[0])),
        group(0x100, &[first, second].concat()),
    ]
    .concat()
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: id,
    }
}
struct Fixture {
    root: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Base.esm"), source(mode)).unwrap();
        Self {
            root,
            names: vec!["Base.esm".into()],
        }
    }
    fn store(&self) -> RecordStore {
        self.open(false)
    }
    fn open(&self, forensic: bool) -> RecordStore {
        let open = if forensic {
            RecordStore::open_nv
        } else {
            RecordStore::open_nv_headers
        };
        open(
            self.root.path(),
            &self.names,
            plugin::Limits {
                inspect_checksum_mismatches: forensic,
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn requests(&self, dialogue: &DialogueSources) -> Vec<Request> {
        vec![
            dialogue
                .request(key(0x100), key(0x300), Some(key(0x400)))
                .unwrap(),
            dialogue.request(key(0x100), key(0x301), None).unwrap(),
        ]
    }
    fn load(
        &self,
        store: &mut RecordStore,
        dialogue: &DialogueSources,
        limits: BatchLimits,
    ) -> fallout_data::Result<ConversationBatch> {
        dialogue.prepare_batch(store, &self.requests(dialogue), &Signatures::new(), limits)
    }
}
#[test]
fn literal_shared_topic_pair_retains_exact_non_utf8_spans_conditions_and_real_fragments() {
    let f = Fixture::new("");
    let mut store = f.store();
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let batch = f.load(&mut store, &dialogue, Default::default()).unwrap();
    let usage = &batch.receipt().usage;
    assert_eq!(usage.requests, 2);
    assert_eq!(usage.maximum_record_bytes, 185);
    assert_eq!(usage.read_bytes, 1266);
    assert_eq!(usage.raw_bytes, 448);
    assert_eq!(usage.fields, 30);
    assert_eq!(usage.section_slots, 58);
    assert_eq!(usage.conditions, 2);
    assert_eq!(usage.fragments, 4);
    let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(())).unwrap();
    batch.validate_sources(&mut store).unwrap();
    drop(store);
    drop(dialogue);
    for (index, source) in batch.conversations().iter().enumerate() {
        let metadata = source.metadata();
        assert_eq!(metadata.topic.record_file_offset, 42);
        assert_eq!(
            metadata.info.record_file_offset,
            if index == 0 { 208 } else { 417 }
        );
        assert_eq!(metadata.info.key, key(0x300 + index as u32));
        assert_eq!(metadata.topic_fields[1].header_decoded_offset, 12);
        assert_eq!(source.topic_bytes(1), Some([255, 128, 84, 0].as_slice()));
        assert_eq!(metadata.responses[0].number, if index == 0 { 9 } else { 2 });
        assert_eq!(metadata.responses[0].fields, [3, 4, 5]);
        assert_eq!(metadata.info_fields[4].header_decoded_offset, 84);
        assert_eq!(metadata.info_fields[5].header_decoded_offset, 94);
        assert_eq!(
            source.subtitle_bytes(0, 0),
            Some(
                if index == 0 {
                    [255, 128, 65, 0]
                } else {
                    [254, 129, 66, 0]
                }
                .as_slice()
            )
        );
        assert_eq!(source.subtitle_bytes(0, 1), Some(b"repeat\0".as_slice()));
        assert_eq!(source.subtitle_bytes(0, 2), None);
        let site = &metadata.conditions.conditions().sites()[0];
        assert_eq!(site.field_decoded_offset(), 20);
        assert_eq!(
            site.raw_bytes(),
            condition(if index == 0 { 65535 } else { 65534 }, 0x400)
        );
        assert_eq!(
            metadata.conditions.ownership().sites()[0].owner_section,
            Some(0)
        );
        assert_eq!(
            metadata
                .fragments
                .iter()
                .map(|frag| frag.key().header_decoded_offset)
                .collect::<Vec<_>>(),
            [107, 149]
        );
        for (fragment, role) in metadata
            .fragments
            .iter()
            .zip([OwnerKind::DialogueBegin, OwnerKind::DialogueEnd])
        {
            assert_eq!(fragment.role(), Some(role));
            let loaded = fragment.resolve(&catalogue).unwrap();
            assert_eq!(loaded.compiled(), Some([0x1d, 0, 0, 0].as_slice()));
            assert_eq!(
                loaded.version().record_file_offset,
                metadata.info.record_file_offset
            );
        }
        assert!(
            !metadata.selection_order_verified
                && !metadata.condition_truth_verified
                && !metadata.speaker_assignment_verified
                && !metadata.fragment_timing_verified
                && !metadata.voice_filename_verified
        );
    }
    let receipt = batch.receipt();
    assert_eq!(receipt.sources.len(), 1);
    assert_eq!(receipt.requests[0].info(), &key(0x300));
    assert_eq!(receipt.requests[1].info(), &key(0x301));
    assert_eq!(receipt.conversation_identities.len(), 2);
    let projection = serde_json::to_string(&batch).unwrap();
    assert!(!projection.contains("repeat\\u0000"));
    assert!(batch.conversations()[1].info_bytes(usize::MAX).is_none());
}
#[test]
fn all_ten_exact_allowances_and_one_under_never_publish_a_successful_prefix() {
    let f = Fixture::new("");
    let mut store = f.store();
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let batch = f.load(&mut store, &dialogue, Default::default()).unwrap();
    let u = &batch.receipt().usage;
    let exact = BatchLimits {
        requests: u.requests,
        record_bytes: u.maximum_record_bytes,
        read_bytes: u.read_bytes,
        raw_bytes: u.raw_bytes,
        fields: u.fields,
        section_slots: u.section_slots,
        conditions: u.conditions,
        fragments: u.fragments,
        source_metadata_bytes: u.source_metadata_bytes,
        retained_bytes: u.retained_bytes,
    };
    assert_eq!(
        f.load(&mut store, &dialogue, exact).unwrap().identity(),
        batch.identity()
    );
    for lower in [
        BatchLimits {
            requests: exact.requests - 1,
            ..exact
        },
        BatchLimits {
            record_bytes: exact.record_bytes - 1,
            ..exact
        },
        BatchLimits {
            read_bytes: exact.read_bytes - 1,
            ..exact
        },
        BatchLimits {
            raw_bytes: exact.raw_bytes - 1,
            ..exact
        },
        BatchLimits {
            fields: exact.fields - 1,
            ..exact
        },
        BatchLimits {
            section_slots: exact.section_slots - 1,
            ..exact
        },
        BatchLimits {
            conditions: exact.conditions - 1,
            ..exact
        },
        BatchLimits {
            fragments: exact.fragments - 1,
            ..exact
        },
        BatchLimits {
            source_metadata_bytes: exact.source_metadata_bytes - 1,
            ..exact
        },
        BatchLimits {
            retained_bytes: exact.retained_bytes - 1,
            ..exact
        },
    ] {
        assert!(f.load(&mut store, &dialogue, lower).is_err());
    }
    let max = BatchLimits::default();
    for invalid in [
        BatchLimits {
            requests: max.requests + 1,
            ..max
        },
        BatchLimits {
            record_bytes: max.record_bytes + 1,
            ..max
        },
        BatchLimits {
            read_bytes: max.read_bytes + 1,
            ..max
        },
        BatchLimits {
            raw_bytes: max.raw_bytes + 1,
            ..max
        },
        BatchLimits {
            fields: max.fields + 1,
            ..max
        },
        BatchLimits {
            section_slots: max.section_slots + 1,
            ..max
        },
        BatchLimits {
            conditions: max.conditions + 1,
            ..max
        },
        BatchLimits {
            fragments: max.fragments + 1,
            ..max
        },
        BatchLimits {
            source_metadata_bytes: max.source_metadata_bytes + 1,
            ..max
        },
        BatchLimits {
            retained_bytes: max.retained_bytes + 1,
            ..max
        },
    ] {
        assert!(f.load(&mut store, &dialogue, invalid).is_err());
    }
    assert_eq!(
        f.load(&mut store, &dialogue, exact).unwrap().identity(),
        batch.identity()
    );
}
#[test]
fn explicit_tuple_uniqueness_and_caller_order_never_select_or_sort_dialogue() {
    let f = Fixture::new("");
    let mut store = f.store();
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let requests = f.requests(&dialogue);
    let normal = f.load(&mut store, &dialogue, Default::default()).unwrap();
    let reverse = dialogue
        .prepare_batch(
            &mut store,
            &[requests[1].clone(), requests[0].clone()],
            &Signatures::new(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(reverse.conversations()[0].metadata().info.key, key(0x301));
    assert_eq!(
        reverse.receipt().conversation_identities[0],
        normal.receipt().conversation_identities[1]
    );
    assert_ne!(reverse.identity(), normal.identity());
    assert!(
        dialogue
            .prepare_batch(&mut store, &[], &Signatures::new(), Default::default())
            .is_err()
    );
    assert!(
        dialogue
            .prepare_batch(
                &mut store,
                &[requests[0].clone(), requests[0].clone()],
                &Signatures::new(),
                Default::default()
            )
            .is_err()
    );
    assert!(
        dialogue
            .prepare_batch(
                &mut store,
                &vec![requests[0].clone(); 9],
                &Signatures::new(),
                Default::default()
            )
            .is_err()
    );
    let unspecified = dialogue.request(key(0x100), key(0x300), None).unwrap();
    let differing = dialogue
        .prepare_batch(
            &mut store,
            &[requests[0].clone(), unspecified],
            &Signatures::new(),
            Default::default(),
        )
        .unwrap();
    assert!(differing.conversations()[0].metadata().speaker.is_some());
    assert!(differing.conversations()[1].metadata().speaker.is_none());
}
#[test]
fn final_speaker_role_framing_and_tainted_source_refuse_whole_batch() {
    for mode in ["ambiguous", "truncated", "tainted"] {
        let f = Fixture::new(mode);
        let mut store = f.open(mode == "tainted");
        let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
        assert!(
            f.load(&mut store, &dialogue, Default::default()).is_err(),
            "{mode}"
        );
    }
    let f = Fixture::new("");
    let mut store = f.store();
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    for speaker in [0x500, 0x999] {
        let requests = [
            f.requests(&dialogue)[0].clone(),
            dialogue
                .request(key(0x100), key(0x301), Some(key(speaker)))
                .unwrap(),
        ];
        assert!(
            dialogue
                .prepare_batch(
                    &mut store,
                    &requests,
                    &Signatures::new(),
                    Default::default()
                )
                .is_err()
        );
    }
    assert!(dialogue.request(key(0x200), key(0x301), None).is_err());
}
#[test]
fn changed_full_sources_and_stale_final_request_cannot_reuse_the_batch_seal() {
    let mut f = Fixture::new("");
    fs::write(f.root.path().join("Other.esm"), header(&[])).unwrap();
    f.names.push("Other.esm".into());
    let mut store = f.store();
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let requests = f.requests(&dialogue);
    let batch = f.load(&mut store, &dialogue, Default::default()).unwrap();
    drop(store);
    for names in [
        vec!["Other.esm".into(), "Base.esm".into()],
        vec!["Base.esm".into()],
        vec!["BASE.esm".into(), "Other.esm".into()],
    ] {
        f.names = names;
        let mut current = f.store();
        assert!(
            dialogue
                .prepare_batch(
                    &mut current,
                    &requests,
                    &Signatures::new(),
                    Default::default()
                )
                .is_err()
        );
        assert!(batch.validate_sources(&mut current).is_err());
        let new = DialogueSources::build(&mut current, Limits::default()).unwrap();
        let first = new.request(key(0x100), key(0x300), None).unwrap();
        assert!(
            new.prepare_batch(
                &mut current,
                &[first, requests[1].clone()],
                &Signatures::new(),
                Default::default()
            )
            .is_err()
        );
    }
    f.names = vec!["Base.esm".into(), "Other.esm".into()];
    fs::write(
        f.root.path().join("Other.esm"),
        [
            header(&[]),
            record(b"ACTI", 0x900, 0, &field(b"ZZZZ", &[79])),
        ]
        .concat(),
    )
    .unwrap();
    let mut changed = f.store();
    assert!(batch.validate_sources(&mut changed).is_err());
    assert!(
        dialogue
            .prepare_batch(
                &mut changed,
                &requests,
                &Signatures::new(),
                Default::default()
            )
            .is_err()
    );
}
#[test]
fn two_master_override_keeps_original_canonical_parent_and_loaded_fragment_identity() {
    let mut f = Fixture::new("");
    fs::write(f.root.path().join("Other.esm"), header(&[])).unwrap();
    let mut data = info(true, "");
    data[16..20].copy_from_slice(&0x01000100_u32.to_le_bytes());
    data[50..54].copy_from_slice(&0x01000400_u32.to_le_bytes());
    fs::write(
        f.root.path().join("Patch.esp"),
        [
            header(&["Other.esm", "Base.esm"]),
            group(0x01000100, &record(b"INFO", 0x01000301, 0, &data)),
        ]
        .concat(),
    )
    .unwrap();
    f.names = vec!["Base.esm".into(), "Other.esm".into(), "Patch.esp".into()];
    let mut store = f.store();
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let batch = f.load(&mut store, &dialogue, Default::default()).unwrap();
    let second = batch.conversations()[1].metadata();
    assert_eq!(second.info.key, key(0x301));
    assert_eq!(second.info.source_plugin, "Patch.esp");
    assert_eq!(second.info.record_file_offset, 125);
    assert_eq!(second.topic.key, key(0x100));
    assert_eq!(
        second
            .links
            .iter()
            .find(|link| link.field_index == 1)
            .unwrap()
            .target
            .raw,
        0x01000100
    );
    let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(())).unwrap();
    for frag in &second.fragments {
        assert_eq!(frag.key().record, key(0x301));
        assert_eq!(
            frag.resolve(&catalogue).unwrap().version().source_plugin,
            "Patch.esp"
        );
    }
}
#[test]
fn compressed_and_xxxx_sources_preserve_logical_spans_and_exact_read_admission() {
    for mode in ["compressed", "xxxx"] {
        let f = Fixture::new(mode);
        let mut store = f.store();
        let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
        let batch = f.load(&mut store, &dialogue, Default::default()).unwrap();
        let second = &batch.conversations()[1];
        assert_eq!(
            second.subtitle_bytes(0, 0),
            Some([254, 129, 66, 0].as_slice())
        );
        assert_eq!(
            second.metadata().info_fields[4].header_decoded_offset,
            if mode == "xxxx" { 94 } else { 84 }
        );
        if mode == "compressed" {
            assert_eq!(second.metadata().info.record_flags, plugin::COMPRESSED);
            assert_eq!(batch.receipt().usage.read_bytes, 1266);
        } else {
            assert_eq!(batch.receipt().usage.read_bytes, 1296);
            assert_eq!(batch.receipt().usage.raw_bytes, 458);
            assert_eq!(
                second.metadata().fragments[0].key().header_decoded_offset,
                117
            );
        }
        let catalogue =
            Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(())).unwrap();
        for frag in &second.metadata().fragments {
            frag.resolve(&catalogue).unwrap();
        }
        let exact = BatchLimits {
            read_bytes: batch.receipt().usage.read_bytes,
            retained_bytes: batch.receipt().usage.retained_bytes,
            ..Default::default()
        };
        assert_eq!(
            f.load(&mut store, &dialogue, exact).unwrap().identity(),
            batch.identity()
        );
        assert!(
            f.load(
                &mut store,
                &dialogue,
                BatchLimits {
                    read_bytes: exact.read_bytes - 1,
                    ..exact
                }
            )
            .is_err()
        );
    }
}
#[test]
fn single_request_compatibility_and_moved_deleted_final_membership_remain_strict() {
    let mut f = Fixture::new("ambiguous");
    let mut store = f.store();
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let requests = f.requests(&dialogue);
    let single = dialogue
        .prepare(
            &mut store,
            &requests[1],
            &Signatures::new(),
            Limits::default(),
        )
        .unwrap();
    assert_eq!(single.metadata().fragments.len(), 3);
    assert!(
        dialogue
            .prepare_batch(
                &mut store,
                &requests,
                &Signatures::new(),
                Default::default()
            )
            .is_err()
    );
    drop(store);
    for (name, parent, flags) in [
        ("Move.esp", 0x200, 0),
        ("Delete.esp", 0x100, plugin::DELETED),
    ] {
        fs::write(
            f.root.path().join(name),
            [
                header(&["Base.esm"]),
                group(parent, &record(b"INFO", 0x301, flags, &info(true, ""))),
            ]
            .concat(),
        )
        .unwrap();
        f.names = vec!["Base.esm".into(), name.into()];
        let mut current = f.store();
        let new = DialogueSources::build(&mut current, Limits::default()).unwrap();
        assert!(new.request(key(0x100), key(0x301), None).is_err());
        assert!(
            dialogue
                .prepare_batch(
                    &mut current,
                    &requests,
                    &Signatures::new(),
                    Default::default()
                )
                .is_err()
        );
    }
}
