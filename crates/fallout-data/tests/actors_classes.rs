use fallout_data::{
    actors::classes::{Catalogue, Limits, Value},
    plugin, record_metadata,
    store::RecordStore,
};
use std::{fs, io::Write, path::Path};

fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn disk(raw: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
    let payload = if flags & plugin::COMPRESSED != 0 {
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(body).unwrap();
        [
            &(body.len() as u32).to_le_bytes()[..],
            &compressor.finish().unwrap(),
        ]
        .concat()
    } else {
        body.to_vec()
    };
    [
        b"CLAS".as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0; 2],
        &payload,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    [
        b"TES4".as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 12],
        &15u16.to_le_bytes(),
        &[0; 2],
        &body,
    ]
    .concat()
}
fn data(first: i32) -> Vec<u8> {
    [
        first.to_le_bytes().as_slice(),
        &i32::MAX.to_le_bytes(),
        &(-1i32).to_le_bytes(),
        &5i32.to_le_bytes(),
        &u32::MAX.to_le_bytes(),
        &0x8000_0000u32.to_le_bytes(),
        &[0x80, 255, 0xaa, 0x55],
    ]
    .concat()
}
fn body(first: i32) -> Vec<u8> {
    [
        field(b"DATA", &data(first)),
        field(b"ATTR", &[0, 255, 1, 2, 3, 4, 5]),
    ]
    .concat()
}
fn source(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn single(path: &Path, body: &[u8], version: u16) -> RecordStore {
    fs::write(
        path.join("FalloutNV.esm"),
        [header(&[]), disk(0x100, 0, version, body)].concat(),
    )
    .unwrap();
    source(path, &["FalloutNV.esm"])
}

#[test]
fn class_words_preserve_signed_extremes_flags_training_unused_and_attributes() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = single(directory.path(), &body(i32::MIN), 14);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let (key, definition) = catalogue.iter().next().unwrap();
    assert!(std::ptr::eq(catalogue.get(key).unwrap(), definition));
    assert_eq!(definition.header.version, 14);
    assert_eq!(
        definition.source.record_file_offset,
        definition.header.offset
    );
    assert_eq!(definition.record().unwrap().payload, body(i32::MIN));
    assert_eq!(
        definition.fields[0].value,
        Value::ClassData {
            tag_skills: [i32::MIN, i32::MAX, -1, 5],
            flags: u32::MAX,
            services: 0x8000_0000,
            teaches: i8::MIN,
            maximum_training_level: 255,
            unused: [0xaa, 0x55]
        }
    );
    assert_eq!(
        definition.fields[1].value,
        Value::Attributes {
            attributes: [0, 255, 1, 2, 3, 4, 5]
        }
    );
    assert!(definition.findings.is_empty());
    assert_eq!(
        catalogue.sources()[0].source_sha256,
        definition.source.sha256
    );
    assert_eq!(
        catalogue.winning_content_sha256(),
        record_metadata::inspect(&store)
            .unwrap()
            .winning_definitions_sha256
    );
}

#[test]
fn ordered_duplicates_extended_unknown_bytes_and_missing_fields_stay_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let extended = [
        field(b"XXXX", &3u32.to_le_bytes()),
        b"UNKN\0\0\x91\x92\x93".to_vec(),
    ]
    .concat();
    let payload = [
        field(b"DATA", &data(1)),
        extended,
        field(b"DATA", &data(2)),
        field(b"ATTR", &[0; 7]),
        field(b"ATTR", &[1; 7]),
    ]
    .concat();
    let mut store = single(directory.path(), &payload, 15);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(
        definition
            .fields
            .iter()
            .map(|field| field.kind)
            .collect::<Vec<_>>(),
        vec![*b"DATA", *b"UNKN", *b"DATA", *b"ATTR", *b"ATTR"]
    );
    assert_eq!(definition.fields[1].decoded_offset, 44);
    assert_eq!(definition.fields[1].bytes, 3);
    assert_eq!(definition.fields[1].value, Value::Opaque);
    assert_eq!(definition.record().unwrap().payload, payload);
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        vec![
            "multiple_class_data_fields",
            "multiple_class_attribute_fields"
        ]
    );
    drop(store);
    let mut missing = single(directory.path(), &field(b"UNKN", &[9]), 15);
    let catalogue = Catalogue::load(&mut missing, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        vec!["missing_class_data_field", "missing_class_attribute_field"]
    );
    assert!(
        definition
            .findings
            .iter()
            .all(|finding| finding.field_decoded_offset.is_none())
    );
    assert_eq!(catalogue.counts().scalar_fields, 0);
}

#[test]
fn unsupported_versions_extents_and_incomplete_frames_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let mut cases = vec![
        (body(1), 13),
        (body(1), 16),
        (b"DAT".to_vec(), 15),
        (b"DATA\x1c\0\0".to_vec(), 15),
        (field(b"XXXX", &28u32.to_le_bytes()), 15),
    ];
    for length in [27, 29] {
        cases.push((field(b"DATA", &vec![0; length]), 15));
    }
    for length in [6, 8] {
        cases.push((field(b"ATTR", &vec![0; length]), 15));
    }
    for (payload, version) in cases {
        let mut store = single(directory.path(), &payload, version);
        assert!(Catalogue::load(&mut store, Limits::default()).is_err());
    }
}

#[test]
fn budgets_apply_to_record_count_individual_decoding_total_decoding_and_fields() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(0x100, plugin::COMPRESSED, 15, &body(1)),
            disk(0x101, 0, 15, &body(2)),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    assert_eq!(catalogue.counts().decoded_bytes, body(1).len() * 2);
    for limits in [
        Limits {
            max_records: 0,
            ..Default::default()
        },
        Limits {
            max_records: 1,
            ..Default::default()
        },
        Limits {
            max_record_bytes: body(1).len() - 1,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: body(1).len() * 2 - 1,
            ..Default::default()
        },
        Limits {
            max_fields: 3,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut store, limits).is_err());
    }
}

#[test]
fn master_relative_overrides_deleted_winners_and_cold_warm_reordering_keep_provenance() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(0x100, 0, 14, &body(1)),
            disk(0x101, 0, 15, &body(2)),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("A.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(0x100, 0, 15, &body(3)),
            disk(0x101, plugin::DELETED, 16, b"unread malformed body"),
            disk(0x0100_0100, plugin::COMPRESSED, 15, &body(4)),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("B.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(0x0100_0100, 0, 14, &body(5)),
        ]
        .concat(),
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut observed = Vec::new();
    let mut hashes = Vec::new();
    for (phase, order) in [
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "B.esm", "A.esm"],
    ]
    .into_iter()
    .enumerate()
    {
        let mut store = RecordStore::open_nv_headers_cached(
            directory.path(),
            &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
            plugin::Limits::default(),
            cache.path(),
        )
        .unwrap();
        assert!(
            store
                .index_cache_report()
                .unwrap()
                .plugins
                .iter()
                .all(|receipt| receipt.reused == (phase != 0))
        );
        let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
        assert_eq!(catalogue.counts().records, 4);
        assert_eq!(catalogue.counts().deleted_records, 1);
        let deleted = catalogue
            .iter()
            .find(|(key, _)| key.origin_plugin == "falloutnv.esm" && key.local_id == 0x101)
            .unwrap()
            .1;
        assert_eq!(deleted.header.version, 16);
        assert_eq!(deleted.source.plugin, "A.esm");
        assert!(deleted.source.decoded_record_sha256.is_none());
        assert!(deleted.record().is_none());
        assert!(deleted.fields.is_empty() && deleted.findings.is_empty());
        let override_definition = catalogue
            .iter()
            .find(|(key, _)| key.origin_plugin == "falloutnv.esm" && key.local_id == 0x100)
            .unwrap()
            .1;
        assert_eq!(override_definition.source.plugin, "A.esm");
        hashes.push(catalogue.winning_content_sha256().to_owned());
        observed.push(
            serde_json::to_value(
                catalogue
                    .iter()
                    .map(|(_, definition)| definition)
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        );
    }
    assert_eq!(observed[0], observed[1]);
    assert_eq!(observed[0], observed[2]);
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(hashes[0], hashes[2]);
}
