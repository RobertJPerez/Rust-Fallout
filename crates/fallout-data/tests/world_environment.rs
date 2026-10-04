//! Independent CELL literals and header-only target witnesses.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    world::environment::{CellEnvironmentSources, Limits},
};
use std::{fs, io::Write};
fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(kind: &[u8; 4], form: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &form.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}
fn header(masters: &[&[u8]]) -> Vec<u8> {
    let mut data = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        data.extend(field(b"MAST", &[*master, &[0]].concat()));
        data.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &data)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: id,
    }
}
fn cell() -> Vec<u8> {
    [
        field(b"DATA", &[1]),
        field(b"XCMO", &[4, 2, 0, 0]),
        field(b"ZZZZ", &[91, 92]),
        field(b"XCCM", &[0, 2, 0, 0]),
        field(b"XEZN", &[2, 2, 0, 0]),
        field(b"XCIM", &[1, 2, 0, 0]),
        field(b"XCAS", &[3, 2, 0, 0]),
    ]
    .concat()
}
const TARGETS: [&[u8; 4]; 5] = [b"CLMT", b"IMGS", b"ECZN", b"ASPC", b"MUSC"];
struct Fixture {
    root: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new(cell: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        let mut bytes = [header(&[]), record(b"CELL", 0x100, 0, cell)].concat();
        for (index, kind) in TARGETS.iter().enumerate() {
            bytes.extend(record(
                kind,
                0x200 + index as u32,
                0,
                &field(b"ZZZZ", &[index as u8]),
            ));
        }
        fs::write(root.path().join("Data/Base.esm"), bytes).unwrap();
        Self {
            root,
            names: vec!["Base.esm".into()],
        }
    }
    fn try_store(&self, forensic: bool) -> fallout_data::Result<RecordStore> {
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
    fn store(&self) -> RecordStore {
        self.try_store(false).unwrap()
    }
    fn load(
        &self,
        store: &mut RecordStore,
        limits: Limits,
    ) -> fallout_data::Result<CellEnvironmentSources> {
        CellEnvironmentSources::load(store, &key(0x100), limits)
    }
    fn patch(&mut self, masters: &[&[u8]], body: &[u8]) {
        fs::write(
            self.root.path().join("Data/Patch.esp"),
            [header(masters), body.to_vec()].concat(),
        )
        .unwrap();
        self.names.push("Patch.esp".into());
    }
}
#[test]
fn all_distinct_literal_links_retain_source_order_ordinals_words_spans_and_expected_headers() {
    let fixture = Fixture::new(&cell());
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    assert_eq!(r.cell.header.offset, 42);
    assert_eq!(r.cell_flags.value, 1);
    assert_eq!(r.links.len(), 5);
    assert_eq!(
        (
            r.usage.records,
            r.usage.fields,
            r.usage.read_bytes,
            r.usage.raw_bytes
        ),
        (6, 7, 65, 50)
    );
    for (link, (kind, id, ordinal, offset, target_header, expected)) in r.links.iter().zip([
        (b"XCMO", 0x204, 1, 7, 255, b"MUSC"),
        (b"XCCM", 0x200, 3, 25, 131, b"CLMT"),
        (b"XEZN", 0x202, 4, 35, 193, b"ECZN"),
        (b"XCIM", 0x201, 5, 45, 162, b"IMGS"),
        (b"XCAS", 0x203, 6, 55, 224, b"ASPC"),
    ]) {
        assert_eq!(link.kind, *kind);
        assert_eq!(link.raw, id);
        assert_eq!(link.physical_field_ordinal, ordinal);
        assert_eq!(link.logical_field_ordinal, ordinal);
        assert_eq!(link.site.decoded_header_offset, offset);
        assert_eq!(link.site.span.decoded_offset, offset + 6);
        assert_eq!(link.site.span.bytes, 4);
        assert_eq!(link.physical_framing_offset, Some(66 + offset as u64));
        assert_eq!(link.framing, field(kind, &id.to_le_bytes()));
        assert_eq!(link.target.key, Some(key(id)));
        assert_eq!(link.target.status, "resolved");
        assert_eq!(
            link.target.expected_kinds,
            vec![String::from_utf8(expected.to_vec()).unwrap()]
        );
        let header = link.source.as_ref().unwrap();
        assert_eq!(header.header.kind, *expected);
        assert_eq!(header.header.offset, target_header);
        assert_eq!(header.source_ordinal, 0);
        assert!(link.header_source_available);
        assert_eq!(
            link.behavior_status,
            "unknown; target body and behavior not decoded"
        );
    }
    assert!(!r.runtime_ready);
}
#[test]
fn absent_zero_missing_deleted_and_wrong_kind_are_distinct_without_world_fallback() {
    let fixture = Fixture::new(&field(b"DATA", &[1]));
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    assert!(sources.receipt().links.is_empty());
    assert_eq!(sources.receipt().usage.records, 1);
    for (raw, kind, flags, status) in [
        (0_u32, b"CLMT", 0, "null"),
        (0x999, b"CLMT", 0, "missing"),
        (0x200, b"CLMT", plugin::DELETED, "deleted"),
        (0x200, b"MUSC", 0, "wrong-record-kind"),
    ] {
        let mut fixture =
            Fixture::new(&[field(b"DATA", &[1]), field(b"XCCM", &raw.to_le_bytes())].concat());
        fixture.patch(&[b"Base.esm"], &record(kind, 0x200, flags, &[1, 2, 3]));
        let sources = fixture
            .load(&mut fixture.store(), Default::default())
            .unwrap();
        let link = &sources.receipt().links[0];
        assert_eq!(link.raw, raw);
        assert_eq!(link.target.status, status);
        assert!(!link.header_source_available);
        assert_eq!(
            link.source.is_some(),
            ["deleted", "wrong-record-kind"].contains(&status)
        );
        if let Some(source) = &link.source {
            assert_eq!(source.source_plugin, "Patch.esp");
            assert_eq!(source.header.offset, 71);
        }
    }
}
#[test]
fn winning_two_master_cell_rewrites_and_target_override_preserve_canonical_identities() {
    let mut fixture = Fixture::new(&cell());
    fs::write(fixture.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    fixture.names.push("Other.esm".into());
    let mut body = cell();
    for offset in [13, 31, 41, 51, 61] {
        body[offset + 3] = 1;
    }
    fixture.patch(
        &[b"Other.esm", b"Base.esm"],
        &[
            record(b"CELL", 0x01000100, 0, &body),
            record(b"CLMT", 0x01000200, 0, &[1, 2, 3]),
        ]
        .concat(),
    );
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    assert_eq!(r.cell.header.offset, 101);
    assert_eq!(r.cell.source_ordinal, 2);
    assert_eq!(r.links[1].raw, 0x01000200);
    assert_eq!(r.links[1].target.key, Some(key(0x200)));
    assert_eq!(r.links[1].source.as_ref().unwrap().header.offset, 190);
    assert_eq!(r.links[1].source.as_ref().unwrap().source_ordinal, 2);
    assert_eq!(r.links[0].target.key, Some(key(0x204)));
    assert_eq!(r.links[0].source.as_ref().unwrap().source_ordinal, 0);
    assert_eq!(r.usage.read_bytes, 65); // Targets stay header-only, including malformed deferred body.
}
#[test]
fn duplicate_wrong_width_truncated_deleted_and_wrong_cell_refuse_without_partial_authority() {
    for kind in [b"XCCM", b"XCIM", b"XEZN", b"XCAS", b"XCMO"] {
        let fixture = Fixture::new(&[cell(), field(kind, &[0; 4])].concat());
        assert!(
            fixture
                .load(&mut fixture.store(), Default::default())
                .is_err()
        );
        for width in [0, 1, 2, 3, 5, 8] {
            let fixture =
                Fixture::new(&[field(b"DATA", &[1]), field(kind, &vec![0; width])].concat());
            assert!(
                fixture
                    .load(&mut fixture.store(), Default::default())
                    .is_err()
            );
        }
    }
    let fixture = Fixture::new(&[field(b"DATA", &[1]), b"XCCM\x04\0\0".to_vec()].concat());
    assert!(
        fixture
            .try_store(false)
            .and_then(|mut store| fixture.load(&mut store, Default::default()))
            .is_err()
    );
    let mut fixture = Fixture::new(&cell());
    fixture.patch(
        &[b"Base.esm"],
        &record(b"CELL", 0x100, plugin::DELETED, &[]),
    );
    assert!(
        fixture
            .load(&mut fixture.store(), Default::default())
            .is_err()
    );
    let fixture = Fixture::new(&cell());
    let mut store = fixture.store();
    for root in [
        key(0x999),
        key(0x200),
        FormKey {
            profile: ProfileId::Fo3Original,
            ..key(0x100)
        },
    ] {
        assert!(CellEnvironmentSources::load(&mut store, &root, Default::default()).is_err());
    }
}
#[test]
fn xxxx_physical_ordinals_and_compressed_decoded_sites_remain_exact() {
    let mut extended = field(b"DATA", &[1]);
    extended.extend(field(b"XXXX", &4_u32.to_le_bytes()));
    extended.extend(b"XCMO\0\0");
    extended.extend([4, 2, 0, 0]);
    extended.extend(&cell()[17..]);
    let fixture = Fixture::new(&extended);
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    let link = &r.links[0];
    assert_eq!(link.logical_field_ordinal, 1);
    assert_eq!(link.physical_field_ordinal, 2);
    assert_eq!(link.site.decoded_header_offset, 17);
    assert_eq!(link.site.span.decoded_offset, 23);
    assert_eq!(link.decoded_framing_offset, 7);
    assert_eq!(link.framing.len(), 20);
    assert_eq!(link.physical_framing_offset, Some(73));
    assert_eq!(r.usage.fields, 8);
    assert_eq!(r.links[1].physical_field_ordinal, 4);
    assert_eq!(r.links[1].logical_field_ordinal, 3);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&extended).unwrap();
    let stored = [
        (extended.len() as u32).to_le_bytes().as_slice(),
        &encoder.finish().unwrap(),
    ]
    .concat();
    let mut fixture = Fixture::new(&cell());
    fixture.patch(
        &[b"Base.esm"],
        &record(b"CELL", 0x100, plugin::COMPRESSED, &stored),
    );
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let link = &sources.receipt().links[0];
    assert!(link.physical_framing_offset.is_none());
    assert_eq!(link.stored_body_offset, 95);
    assert_eq!(link.site.decoded_header_offset, 17);
    assert_eq!(link.stored_body_bytes, stored.len() as u32);
}
#[test]
fn all_eight_lowerable_allowances_accept_exact_and_one_under_refuses_then_retry_is_identical() {
    let fixture = Fixture::new(&cell());
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    let u = &sources.receipt().usage;
    let exact = Limits {
        sources: u.sources,
        records: u.records,
        fields: u.fields,
        links: u.links,
        record_bytes: 65,
        read_bytes: u.read_bytes,
        raw_bytes: u.raw_bytes,
        metadata_bytes: u.metadata_bytes,
    };
    let identity = sources.identity().to_owned();
    assert_eq!(
        fixture.load(&mut store, exact).unwrap().identity(),
        identity
    );
    let mut limits = Vec::new();
    macro_rules! under {
        ($field:ident) => {
            limits.push(Limits {
                $field: exact.$field - 1,
                ..exact
            });
        };
    }
    under!(sources);
    under!(records);
    under!(fields);
    under!(links);
    under!(record_bytes);
    under!(read_bytes);
    under!(raw_bytes);
    under!(metadata_bytes);
    for limit in limits {
        assert!(fixture.load(&mut store, limit).is_err());
    }
    assert_eq!(
        fixture.load(&mut store, exact).unwrap().identity(),
        identity
    );
    assert!(
        fixture
            .load(
                &mut store,
                Limits {
                    links: 6,
                    ..Default::default()
                }
            )
            .is_err()
    );
}
#[test]
fn full_order_name_count_and_deferred_bytes_are_required_for_reuse() {
    let mut fixture = Fixture::new(&cell());
    fixture.patch(&[b"Base.esm"], &[]);
    fs::write(fixture.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    fixture.names.push("Other.esm".into());
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    drop(store);
    fixture.names.swap(1, 2);
    assert!(sources.validate_sources(&mut fixture.store()).is_err());
    fixture.names.swap(1, 2);
    fixture.names.pop();
    assert!(sources.validate_sources(&mut fixture.store()).is_err());
    fixture.names.push("Other.esm".into());
    fs::copy(
        fixture.root.path().join("Data/Other.esm"),
        fixture.root.path().join("Data/Renamed.esm"),
    )
    .unwrap();
    fixture.names[2] = "Renamed.esm".into();
    assert!(sources.validate_sources(&mut fixture.store()).is_err());
    fixture.names[2] = "Other.esm".into();
    fs::write(
        fixture.root.path().join("Data/Other.esm"),
        [header(&[]), record(b"STAT", 0x900, 0, &[1, 2, 3])].concat(),
    )
    .unwrap();
    assert!(sources.validate_sources(&mut fixture.store()).is_err());
}
#[test]
fn recovered_cell_checksum_body_cannot_construct_environment_authority() {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&cell()).unwrap();
    let mut stream = encoder.finish().unwrap();
    *stream.last_mut().unwrap() ^= 1;
    let stored = [65_u32.to_le_bytes().as_slice(), &stream].concat();
    let mut fixture = Fixture::new(&cell());
    fixture.patch(
        &[b"Base.esm"],
        &record(b"CELL", 0x100, plugin::COMPRESSED, &stored),
    );
    assert!(
        fixture
            .try_store(false)
            .and_then(|mut store| fixture.load(&mut store, Default::default()))
            .is_err()
    );
    let mut forensic = fixture.try_store(true).unwrap();
    assert!(fixture.load(&mut forensic, Default::default()).is_err());
}
