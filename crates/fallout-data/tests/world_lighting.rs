//! Independent byte literals exercise the protected winning source factory.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    world::lighting::{ByteColor, CellLightingSources, Limits},
};
use std::{fs, io::Write};
fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], raw: u32, flags: u32, bytes: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(bytes.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}
fn header(master: bool) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    if master {
        body.extend(field(b"MAST", b"Base.esm\0"));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
// Three RGB+unused colors followed by independent float/signed words. Includes
// a NaN payload, negative zero, both signed integer extremes and infinity.
const LIGHT: [u8; 40] = [
    1, 2, 3, 0xf1, 4, 5, 6, 0xf2, 7, 8, 9, 0xf3, 0x45, 0x23, 0xc1, 0x7f, 0, 0, 0, 0x80, 0, 0, 0,
    0x80, 0xff, 0xff, 0xff, 0x7f, 0, 0, 0xc0, 0x3f, 0, 0, 0x80, 0x7f, 0, 0, 0, 0x40,
];
fn key(raw: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: raw,
    }
}
struct Fixture {
    root: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new(cell: &[u8], template: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        fs::write(
            root.path().join("Data/Base.esm"),
            [
                header(false),
                record(b"CELL", 0x100, 0, cell),
                template.to_vec(),
            ]
            .concat(),
        )
        .unwrap();
        Self {
            root,
            names: vec!["Base.esm".into()],
        }
    }
    fn open(&self, forensic: bool) -> RecordStore {
        self.try_open(forensic).unwrap()
    }
    fn try_open(&self, forensic: bool) -> fallout_data::Result<RecordStore> {
        let open = if forensic {
            RecordStore::open_nv
        } else {
            RecordStore::open_nv_headers
        };
        open(
            &self.root.path().join("Data"),
            &self.names,
            plugin::Limits {
                inspect_checksum_mismatches: forensic,
                ..Default::default()
            },
        )
    }
    fn patch(&mut self, bytes: &[u8]) {
        fs::write(
            self.root.path().join("Data/Patch.esp"),
            [header(true), bytes.to_vec()].concat(),
        )
        .unwrap();
        self.names.push("Patch.esp".into());
    }
}
fn cell_body(light: &[u8], ltmp: u32) -> Vec<u8> {
    [
        field(b"DATA", &[1]),
        field(b"XCLL", light),
        field(b"LTMP", &ltmp.to_le_bytes()),
        field(b"LNAM", &[0x09, 0x01, 0, 0x80]),
    ]
    .concat()
}
fn template() -> Vec<u8> {
    record(
        b"LGTM",
        0x200,
        0,
        &[field(b"EDID", b"lt\0"), field(b"DATA", &LIGHT)].concat(),
    )
}
#[test]
fn literal_full_and_all_justified_short_lighting_prefixes_preserve_every_word_and_span() {
    for bytes in [28, 32, 36, 40] {
        let fixture = Fixture::new(&cell_body(&LIGHT[..bytes], 0x200), &template());
        let mut store = fixture.open(false);
        let prepared =
            CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
        let receipt = prepared.receipt();
        let raw = receipt.xcll.as_ref().unwrap();
        let light = &raw.value;
        assert_eq!(
            light.ambient,
            ByteColor {
                red: 1,
                green: 2,
                blue: 3,
                unused: 0xf1
            }
        );
        assert_eq!(
            light.directional,
            ByteColor {
                red: 4,
                green: 5,
                blue: 6,
                unused: 0xf2
            }
        );
        assert_eq!(
            light.fog,
            ByteColor {
                red: 7,
                green: 8,
                blue: 9,
                unused: 0xf3
            }
        );
        assert_eq!(light.fog_near_word, 0x7fc12345);
        assert_eq!(light.fog_far_word, 0x80000000);
        assert_eq!(light.rotation_xy_word, 0x80000000);
        assert_eq!(light.rotation_xy, i32::MIN);
        assert_eq!(light.rotation_z_word, 0x7fffffff);
        assert_eq!(light.rotation_z, i32::MAX);
        assert_eq!(
            light.directional_fade_word,
            (bytes >= 32).then_some(0x3fc00000)
        );
        assert_eq!(
            light.fog_clip_distance_word,
            (bytes >= 36).then_some(0x7f800000)
        );
        assert_eq!(light.fog_power_word, (bytes >= 40).then_some(0x40000000));
        assert_eq!(raw.site.decoded_header_offset, 7);
        assert_eq!(raw.site.span.decoded_offset, 13);
        assert_eq!(raw.site.span.bytes, bytes);
        assert_eq!(raw.decoded_framing_offset, 7);
        assert_eq!(raw.physical_framing_offset, Some(73));
        assert_eq!(raw.framing, field(b"XCLL", &LIGHT[..bytes]));
        let ltmp = receipt.ltmp.as_ref().unwrap();
        let lnam = receipt.lnam.as_ref().unwrap();
        assert_eq!(ltmp.value, 0x200);
        assert_eq!(ltmp.site.decoded_header_offset, 13 + bytes);
        assert_eq!(lnam.value, 0x80000109);
        assert_eq!(lnam.site.decoded_header_offset, 23 + bytes);
        let linked = receipt.template.as_ref().unwrap();
        assert_eq!(linked.input_status, "resolved");
        assert_eq!(linked.target.key, Some(key(0x200)));
        assert_eq!(linked.target.expected_kinds, ["LGTM"]);
        assert_eq!(
            linked.source.as_ref().unwrap().header.offset,
            99 + bytes as u64
        );
        let data = linked.lighting.as_ref().unwrap();
        assert_eq!(data.site.decoded_header_offset, 9);
        assert_eq!(data.site.span.decoded_offset, 15);
        assert_eq!(data.site.span.bytes, 40);
        assert_eq!(data.value.fog_power_word, Some(0x40000000));
        assert!(prepared.source_inputs_available());
        assert!(prepared.validate_sources(&mut store).is_ok());
        assert!(!receipt.runtime_ready);
        assert_eq!(receipt.cell.header.offset, 42);
        let view = serde_json::to_value(&prepared).unwrap();
        assert_eq!(view["xcll"]["value"]["fog_near_word"], 0x7fc12345_u32);
    }
}
#[test]
fn extended_framing_and_compression_preserve_exact_source_extents_without_invented_file_offsets() {
    let extended = [
        field(b"DATA", &[1]),
        field(b"XXXX", &40_u32.to_le_bytes()),
        b"XCLL\0\0".to_vec(),
        LIGHT.to_vec(),
    ]
    .concat();
    let fixture = Fixture::new(&extended, &[]);
    let mut store = fixture.open(false);
    let prepared = CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
    let raw = prepared.receipt().xcll.as_ref().unwrap();
    assert_eq!(raw.site.decoded_header_offset, 17);
    assert_eq!(raw.site.span.decoded_offset, 23);
    assert_eq!(raw.decoded_framing_offset, 7);
    assert_eq!(raw.physical_framing_offset, Some(73));
    assert_eq!(raw.framing, &extended[7..]);
    assert_eq!(prepared.receipt().usage.raw_bytes, 56);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&extended).unwrap();
    let compressed = [
        (extended.len() as u32).to_le_bytes().as_slice(),
        &encoder.finish().unwrap(),
    ]
    .concat();
    let mut fixture = Fixture::new(&field(b"DATA", &[1]), &[]);
    fixture.patch(&record(b"CELL", 0x100, plugin::COMPRESSED, &compressed));
    let mut store = fixture.open(false);
    let prepared = CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
    let raw = prepared.receipt().xcll.as_ref().unwrap();
    assert!(raw.physical_framing_offset.is_none());
    assert_eq!(raw.stored_body_offset, 95);
    assert_eq!(raw.stored_body_bytes, compressed.len() as u32);
    assert_eq!(raw.framing, &extended[7..]);
}
#[test]
fn template_missing_null_deleted_wrong_kind_and_absent_data_are_explicit_without_defaults() {
    for (raw, kind, flags, body, status) in [
        (0, b"LGTM", 0, field(b"DATA", &LIGHT), "null"),
        (0x999, b"LGTM", 0, field(b"DATA", &LIGHT), "missing"),
        (
            0x200,
            b"LGTM",
            plugin::DELETED,
            field(b"DATA", &LIGHT),
            "deleted",
        ),
        (
            0x200,
            b"STAT",
            0,
            field(b"DATA", &LIGHT),
            "wrong-record-kind",
        ),
        (
            0x200,
            b"LGTM",
            0,
            field(b"EDID", b"empty\0"),
            "missing-data",
        ),
    ] {
        let fixture = Fixture::new(&cell_body(&LIGHT, raw), &record(kind, 0x200, flags, &body));
        let mut store = fixture.open(false);
        let prepared =
            CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
        let target = prepared.receipt().template.as_ref().unwrap();
        assert_eq!(target.input_status, status);
        assert!(target.lighting.is_none());
        assert_eq!(prepared.source_inputs_available(), status == "null");
        if ["deleted", "wrong-record-kind"].contains(&status) {
            assert!(target.source.as_ref().unwrap().decoded_sha256.is_none());
        }
    }
    for body in [
        field(b"DATA", &[1]),
        [field(b"DATA", &[1]), field(b"LNAM", &[0xff; 4])].concat(),
    ] {
        let fixture = Fixture::new(&body, &[]);
        let mut store = fixture.open(false);
        let prepared =
            CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
        assert!(prepared.receipt().xcll.is_none());
        assert!(prepared.receipt().ltmp.is_none());
        assert!(prepared.receipt().template.is_none());
        assert!(!prepared.source_inputs_available());
    }
}
#[test]
fn winning_template_override_resolves_cell_master_context_without_old_body_fallback() {
    let mut fixture = Fixture::new(&cell_body(&LIGHT, 0x200), &template());
    let mut replacement = LIGHT;
    replacement[..4].copy_from_slice(&[90, 80, 70, 60]);
    fixture.patch(&record(b"LGTM", 0x200, 0, &field(b"DATA", &replacement)));
    let mut store = fixture.open(false);
    let prepared = CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
    let linked = prepared.receipt().template.as_ref().unwrap();
    assert_eq!(linked.target.key, Some(key(0x200)));
    let source = linked.source.as_ref().unwrap();
    assert_eq!(source.source_plugin, "Patch.esp");
    assert_eq!(source.header.offset, 71);
    assert_eq!(source.source_ordinal, 1);
    assert_eq!(
        linked.lighting.as_ref().unwrap().value.ambient,
        ByteColor {
            red: 90,
            green: 80,
            blue: 70,
            unused: 60
        }
    );
}
#[test]
fn duplicate_scalar_truncated_and_unsupported_lighting_and_template_layouts_refuse_whole_request() {
    let mut bodies = vec![];
    for size in [0, 24, 27, 29, 30, 31, 33, 35, 37, 39, 41] {
        bodies.push([field(b"DATA", &[1]), field(b"XCLL", &vec![0; size])].concat());
    }
    for kind in [b"XCLL", b"LTMP", b"LNAM"] {
        let data = if kind == b"XCLL" {
            LIGHT.as_slice()
        } else {
            &[0; 4]
        };
        bodies.push([cell_body(&LIGHT, 0x200), field(kind, data)].concat());
    }
    for kind in [b"LTMP", b"LNAM"] {
        bodies.push([field(b"DATA", &[1]), field(kind, &[0; 3])].concat());
    }
    bodies.push([field(b"DATA", &[1]), b"XCLL\x28\0\0".to_vec()].concat());
    for body in bodies {
        let fixture = Fixture::new(&body, &template());
        let result = fixture.try_open(false).and_then(|mut store| {
            CellLightingSources::load(&mut store, &key(0x100), Default::default())
        });
        assert!(result.is_err());
    }
    for data in [
        field(b"DATA", &LIGHT[..36]),
        [field(b"DATA", &LIGHT), field(b"DATA", &LIGHT)].concat(),
    ] {
        let fixture = Fixture::new(&cell_body(&LIGHT, 0x200), &record(b"LGTM", 0x200, 0, &data));
        let mut store = fixture.open(false);
        assert!(CellLightingSources::load(&mut store, &key(0x100), Default::default()).is_err());
    }
}
#[test]
fn every_construction_allowance_accepts_exact_and_one_under_refuses_then_retries() {
    let fixture = Fixture::new(&cell_body(&LIGHT, 0x200), &template());
    let mut store = fixture.open(false);
    let prepared = CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
    let u = &prepared.receipt().usage;
    let exact = Limits {
        sources: u.sources,
        records: u.records,
        fields: u.fields,
        record_bytes: 73,
        read_bytes: u.read_bytes,
        raw_bytes: u.raw_bytes,
        metadata_bytes: u.metadata_bytes,
        template_links: u.template_links,
    };
    let identity = prepared.identity().to_owned();
    assert_eq!(
        CellLightingSources::load(&mut store, &key(0x100), exact)
            .unwrap()
            .identity(),
        identity
    );
    for limits in [
        Limits {
            sources: exact.sources - 1,
            ..exact
        },
        Limits {
            records: exact.records - 1,
            ..exact
        },
        Limits {
            fields: exact.fields - 1,
            ..exact
        },
        Limits {
            record_bytes: exact.record_bytes - 1,
            ..exact
        },
        Limits {
            read_bytes: exact.read_bytes - 1,
            ..exact
        },
        Limits {
            raw_bytes: exact.raw_bytes - 1,
            ..exact
        },
        Limits {
            metadata_bytes: exact.metadata_bytes - 1,
            ..exact
        },
        Limits {
            template_links: exact.template_links - 1,
            ..exact
        },
    ] {
        assert!(CellLightingSources::load(&mut store, &key(0x100), limits).is_err());
    }
    assert_eq!(
        CellLightingSources::load(&mut store, &key(0x100), exact)
            .unwrap()
            .identity(),
        identity
    );
    assert!(
        CellLightingSources::load(
            &mut store,
            &key(0x100),
            Limits {
                records: 3,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn source_request_refuses_changed_names_order_count_and_deferred_bytes_and_wrong_cell() {
    let mut fixture = Fixture::new(&cell_body(&LIGHT, 0x200), &template());
    fixture.patch(&[]);
    fs::write(fixture.root.path().join("Data/Other.esm"), header(false)).unwrap();
    fixture.names.push("Other.esm".into());
    let mut store = fixture.open(false);
    let prepared = CellLightingSources::load(&mut store, &key(0x100), Default::default()).unwrap();
    for key in [
        key(0x999),
        key(0x200),
        FormKey {
            profile: ProfileId::Fo3Original,
            ..key(0x100)
        },
    ] {
        assert!(CellLightingSources::load(&mut store, &key, Default::default()).is_err());
    }
    drop(store);
    fixture.names.swap(1, 2);
    assert!(prepared.validate_sources(&mut fixture.open(false)).is_err());
    fixture.names.swap(1, 2);
    fixture.names.pop();
    assert!(prepared.validate_sources(&mut fixture.open(false)).is_err());
    fixture.names.push("Other.esm".into());
    fs::copy(
        fixture.root.path().join("Data/Other.esm"),
        fixture.root.path().join("Data/Renamed.esm"),
    )
    .unwrap();
    fixture.names[2] = "Renamed.esm".into();
    assert!(prepared.validate_sources(&mut fixture.open(false)).is_err());
    fixture.names[2] = "Other.esm".into();
    let path = fixture.root.path().join("Data/Other.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"STAT", 0x900, 0, &[1, 2, 3]));
    fs::write(path, bytes).unwrap();
    assert!(prepared.validate_sources(&mut fixture.open(false)).is_err());
}
#[test]
fn checksum_tainted_cell_or_template_is_inspectable_but_cannot_prepare_source_authority() {
    for template_tainted in [false, true] {
        let body = if template_tainted {
            field(b"DATA", &LIGHT)
        } else {
            cell_body(&LIGHT, 0x200)
        };
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&body).unwrap();
        let mut stream = encoder.finish().unwrap();
        let last = stream.len() - 1;
        stream[last] ^= 1;
        let compressed = [(body.len() as u32).to_le_bytes().as_slice(), &stream].concat();
        let mut fixture = Fixture::new(&cell_body(&LIGHT, 0x200), &template());
        fixture.patch(&record(
            if template_tainted { b"LGTM" } else { b"CELL" },
            if template_tainted { 0x200 } else { 0x100 },
            plugin::COMPRESSED,
            &compressed,
        ));
        assert!(
            fixture
                .try_open(false)
                .and_then(|mut store| CellLightingSources::load(
                    &mut store,
                    &key(0x100),
                    Default::default()
                ))
                .is_err()
        );
        let mut forensic = fixture.open(true);
        assert!(CellLightingSources::load(&mut forensic, &key(0x100), Default::default()).is_err());
    }
}
