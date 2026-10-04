//! Independent CELL literals and header-only target witnesses.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    world::regions::{CellRegionSources, Limits},
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
        field(b"DATA", &[2]),
        field(b"XCLR", &[1, 2, 0, 0, 0, 2, 0, 0, 1, 2, 0, 0]),
        field(b"ZZZZ", &[91, 92]),
        field(b"XCLR", &[2, 2, 0, 0, 0, 2, 0, 0]),
        field(b"XCLR", &[]),
    ]
    .concat()
}
const TARGETS: [&[u8; 4]; 3] = [b"REGN", b"REGN", b"REGN"];
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
    ) -> fallout_data::Result<CellRegionSources> {
        CellRegionSources::load(store, &key(0x100), limits)
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
fn repeated_interleaved_elements_multiple_and_empty_occurrences_retain_literal_disk_order() {
    let fixture = Fixture::new(&cell());
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    assert_eq!(r.cell.header.offset, 42);
    assert_eq!(r.cell_flags.value, 2);
    assert_eq!(
        (
            r.usage.records,
            r.usage.fields,
            r.usage.occurrences,
            r.usage.elements,
            r.usage.read_bytes,
            r.usage.raw_bytes
        ),
        (6, 5, 3, 5, 53, 38)
    );
    for (occurrence, (ordinal, offset, words)) in r.occurrences.iter().zip([
        (1, 7, vec![0x201_u32, 0x200, 0x201]),
        (3, 33, vec![0x202, 0x200]),
        (4, 47, vec![]),
    ]) {
        assert_eq!(
            occurrence.occurrence_ordinal,
            if offset == 7 {
                0
            } else if offset == 33 {
                1
            } else {
                2
            }
        );
        assert_eq!(occurrence.physical_field_ordinal, ordinal);
        assert_eq!(occurrence.logical_field_ordinal, ordinal);
        assert_eq!(occurrence.site.decoded_header_offset, offset);
        assert_eq!(occurrence.site.span.decoded_offset, offset + 6);
        assert_eq!(occurrence.site.span.bytes, words.len() * 4);
        assert_eq!(occurrence.physical_framing_offset, Some(66 + offset as u64));
        assert_eq!(
            occurrence.framing,
            field(
                b"XCLR",
                &words
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect::<Vec<_>>()
            )
        );
        assert_eq!(occurrence.elements.len(), words.len());
        for (index, (element, id)) in occurrence.elements.iter().zip(words).enumerate() {
            assert_eq!(element.element_ordinal, index);
            assert_eq!(element.raw, id);
            assert_eq!(element.target.key, Some(key(id)));
            assert_eq!(element.target.status, "resolved");
            assert_eq!(element.target.expected_kinds, vec!["REGN"]);
            assert_eq!(element.span.decoded_offset, offset + 6 + 4 * index);
            assert_eq!(element.span.bytes, 4);
            assert_eq!(
                element.physical_payload_offset,
                Some(72 + offset as u64 + 4 * index as u64)
            );
            let source = element.source.as_ref().unwrap();
            assert_eq!(source.header.kind, *b"REGN");
            assert_eq!(
                source.header.offset,
                match id {
                    0x200 => 119,
                    0x201 => 150,
                    0x202 => 181,
                    _ => panic!(),
                }
            );
            assert_eq!(source.source_ordinal, 0);
            assert!(element.header_source_available);
            assert_eq!(
                element.behavior_status,
                "unknown; REGN body and behavior not decoded"
            );
        }
    }
    assert!(!r.runtime_ready);
}
#[test]
fn absent_empty_null_missing_deleted_wrong_kind_and_resolved_remain_separate() {
    let fixture = Fixture::new(&field(b"DATA", &[1]));
    let r = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    assert!(r.receipt().occurrences.is_empty());
    assert_eq!(r.receipt().usage.elements, 0);
    let body = [
        field(b"DATA", &[1]),
        field(
            b"XCLR",
            &[0_u32, 0x999, 0x200, 0x201, 0x202]
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        ),
    ]
    .concat();
    let mut fixture = Fixture::new(&body);
    fixture.patch(
        &[b"Base.esm"],
        &[
            record(b"REGN", 0x200, plugin::DELETED, &[1, 2, 3]),
            record(b"MUSC", 0x201, 0, &[1, 2, 3]),
        ]
        .concat(),
    );
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    for (element, status) in sources.receipt().occurrences[0].elements.iter().zip([
        "null",
        "missing",
        "deleted",
        "wrong-record-kind",
        "resolved",
    ]) {
        assert_eq!(element.target.status, status);
        assert_eq!(element.header_source_available, status == "resolved");
        assert_eq!(
            element.source.is_some(),
            !["null", "missing"].contains(&status)
        );
    }
    assert_eq!(sources.receipt().usage.read_bytes, 33); // Deferred malformed target bodies remain unread.
}
#[test]
fn two_master_winning_cell_remaps_every_repeat_and_preserves_target_override() {
    let mut fixture = Fixture::new(&cell());
    fs::write(fixture.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    fixture.names.push("Other.esm".into());
    let mut body = cell();
    for offset in [13, 17, 21, 39, 43] {
        body[offset + 3] = 1;
    }
    fixture.patch(
        &[b"Other.esm", b"Base.esm"],
        &[
            record(b"CELL", 0x01000100, 0, &body),
            record(b"REGN", 0x01000200, 0, &[1, 2, 3]),
        ]
        .concat(),
    );
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    assert_eq!(r.cell.header.offset, 101);
    assert_eq!(r.cell.source_ordinal, 2);
    assert_eq!(r.usage.read_bytes, 53);
    let elements = r
        .occurrences
        .iter()
        .flat_map(|o| &o.elements)
        .collect::<Vec<_>>();
    for (element, id) in elements.iter().zip([0x201, 0x200, 0x201, 0x202, 0x200]) {
        assert_eq!(element.raw, 0x01000000 + id);
        assert_eq!(element.target.key, Some(key(id)));
    }
    for index in [1, 4] {
        assert_eq!(elements[index].source.as_ref().unwrap().header.offset, 178);
        assert_eq!(elements[index].source.as_ref().unwrap().source_ordinal, 2);
    }
    assert_eq!(elements[0].source.as_ref().unwrap().source_ordinal, 0);
}
#[test]
fn nonword_tail_truncation_missing_master_and_wrong_roots_refuse_self_selectors_remain_valid() {
    for width in [1, 2, 3, 5, 7] {
        let fixture =
            Fixture::new(&[field(b"DATA", &[1]), field(b"XCLR", &vec![0; width])].concat());
        assert!(
            fixture
                .load(&mut fixture.store(), Default::default())
                .is_err()
        );
    }
    let fixture = Fixture::new(&[field(b"DATA", &[1]), b"XCLR\x04\0\0".to_vec()].concat());
    assert!(
        fixture
            .try_store(false)
            .and_then(|mut s| fixture.load(&mut s, Default::default()))
            .is_err()
    );
    let fixture = Fixture::new(&[field(b"DATA", &[1]), field(b"XCLR", &[0, 2, 0, 2])].concat());
    let mut s = fixture.store();
    let sources = fixture.load(&mut s, Default::default()).unwrap();
    let element = &sources.receipt().occurrences[0].elements[0];
    assert_eq!(element.raw, 0x02000200);
    assert_eq!(element.target.key, Some(key(0x200)));
    assert_eq!(element.target.status, "resolved");
    let error = fixture
        .load(
            &mut s,
            Limits {
                elements: 0,
                ..Default::default()
            },
        )
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("array elements/source links budget"),
        "{error}"
    ); // Array precharge precedes per-element resolution/allocation.
    let mut fixture = Fixture::new(&cell());
    fixture.patch(&[b"Missing.esm"], &record(b"CELL", 0x100, 0, &cell()));
    assert!(fixture.try_store(false).is_err());
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
        assert!(CellRegionSources::load(&mut store, &root, Default::default()).is_err());
    }
}
#[test]
fn extended_array_physical_ordinals_and_compressed_spans_do_not_invent_offsets() {
    let mut extended = field(b"DATA", &[2]);
    extended.extend(field(b"XXXX", &12_u32.to_le_bytes()));
    extended.extend(b"XCLR\0\0");
    extended.extend([1, 2, 0, 0, 0, 2, 0, 0, 1, 2, 0, 0]);
    extended.extend(&cell()[25..]);
    let fixture = Fixture::new(&extended);
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    let o = &r.occurrences[0];
    assert_eq!(r.usage.fields, 6);
    assert_eq!(o.physical_field_ordinal, 2);
    assert_eq!(o.logical_field_ordinal, 1);
    assert_eq!(o.site.decoded_header_offset, 17);
    assert_eq!(o.site.span.decoded_offset, 23);
    assert_eq!(o.decoded_framing_offset, 7);
    assert_eq!(o.framing.len(), 28);
    assert_eq!(o.physical_framing_offset, Some(73));
    assert_eq!(o.elements[2].span.decoded_offset, 31);
    assert_eq!(o.elements[2].physical_payload_offset, Some(97));
    assert_eq!(r.occurrences[1].physical_field_ordinal, 4);
    assert_eq!(r.occurrences[1].logical_field_ordinal, 3);
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
    let o = &sources.receipt().occurrences[0];
    assert!(o.physical_framing_offset.is_none());
    assert!(
        o.elements
            .iter()
            .all(|e| e.physical_payload_offset.is_none())
    );
    assert_eq!(o.stored_body_offset, 95);
    assert_eq!(o.stored_body_bytes, stored.len() as u32);
    assert_eq!(o.site.decoded_header_offset, 17);
}
#[test]
fn all_nine_exact_lowerable_allowances_and_one_under_refusal_leave_retry_identical() {
    let fixture = Fixture::new(&cell());
    let mut store = fixture.store();
    let source = fixture.load(&mut store, Default::default()).unwrap();
    let u = &source.receipt().usage;
    let exact = Limits {
        sources: u.sources,
        records: u.records,
        fields: u.fields,
        occurrences: u.occurrences,
        elements: u.elements,
        record_bytes: 53,
        read_bytes: u.read_bytes,
        raw_bytes: u.raw_bytes,
        metadata_bytes: u.metadata_bytes,
    };
    let identity = source.identity().to_owned();
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
    under!(occurrences);
    under!(elements);
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
                    elements: 4097,
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
fn recovered_cell_checksum_body_cannot_construct_region_authority() {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&cell()).unwrap();
    let mut stream = encoder.finish().unwrap();
    *stream.last_mut().unwrap() ^= 1;
    let stored = [53_u32.to_le_bytes().as_slice(), &stream].concat();
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
