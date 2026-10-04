//! Independent CELL literals and header-only target witnesses.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    world::linked::{Limits, PlacedLinkedSources},
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
        field(b"NAME", &[0, 5, 0, 0]),
        field(b"XCLP", &[1, 2, 3, 170, 4, 5, 6, 187]),
        field(b"ZZZZ", &[91, 92]),
        field(b"XLKR", &[0, 2, 0, 0]),
        field(b"DATA", &[0; 24]),
    ]
    .concat()
}
const TARGETS: [&[u8; 4]; 7] = [
    b"PGRE", b"PMIS", b"PBEA", b"REFR", b"ACHR", b"ACRE", b"PLYR",
];
struct Fixture {
    root: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new(cell: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        let mut bytes = [header(&[]), record(b"REFR", 0x100, 0, cell)].concat();
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
    ) -> fallout_data::Result<PlacedLinkedSources> {
        PlacedLinkedSources::load(store, &key(0x100), limits)
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
fn independent_link_and_color_literals_retain_raw_unused_bytes_order_spans_and_target_header() {
    let f = Fixture::new(&cell());
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let r = source.receipt();
    assert_eq!(r.placed.header.offset, 42);
    assert_eq!(r.placement.base.value, 0x500);
    assert_eq!(
        (
            r.usage.records,
            r.usage.fields,
            r.usage.links,
            r.usage.read_bytes,
            r.usage.raw_bytes
        ),
        (2, 5, 1, 72, 24)
    );
    assert_eq!(r.placement.unhandled_fields["XLKR"], 1);
    assert_eq!(r.placement.unhandled_fields["XCLP"], 1);
    let color = r.color.as_ref().unwrap();
    assert_eq!(color.raw, [1, 2, 3, 170, 4, 5, 6, 187]);
    assert_eq!(color.field.physical_field_ordinal, 1);
    assert_eq!(color.field.logical_field_ordinal, 1);
    assert_eq!(color.field.site.decoded_header_offset, 10);
    assert_eq!(color.field.site.span.decoded_offset, 16);
    assert_eq!(color.field.site.span.bytes, 8);
    assert_eq!(color.field.physical_framing_offset, Some(76));
    assert_eq!(
        color.field.framing,
        field(b"XCLP", &[1, 2, 3, 170, 4, 5, 6, 187])
    );
    let link = r.link.as_ref().unwrap();
    assert_eq!(link.raw, 0x200);
    assert_eq!(link.target.key, Some(key(0x200)));
    assert_eq!(link.target.status, "resolved");
    assert_eq!(
        link.target.expected_kinds,
        vec!["REFR", "ACRE", "ACHR", "PGRE", "PMIS", "PBEA", "PLYR"]
    );
    assert_eq!(link.field.physical_field_ordinal, 3);
    assert_eq!(link.field.logical_field_ordinal, 3);
    assert_eq!(link.field.site.decoded_header_offset, 32);
    assert_eq!(link.field.site.span.decoded_offset, 38);
    assert_eq!(link.field.physical_framing_offset, Some(98));
    assert_eq!(link.field.framing, field(b"XLKR", &[0, 2, 0, 0]));
    let header = link.source.as_ref().unwrap();
    assert_eq!(header.header.kind, *b"PGRE");
    assert_eq!(header.header.offset, 138);
    assert!(link.header_source_available);
    assert_eq!(
        link.behavior_status,
        "unknown; target body, placement and binding not evaluated"
    );
    assert_eq!(color.behavior_status, "unknown; source colors not applied");
    assert!(!r.runtime_ready);
}
#[test]
fn every_pinned_header_domain_remains_supported_without_target_placement_decoding() {
    for (index, kind) in TARGETS.iter().enumerate() {
        let mut body = cell();
        body[38..42].copy_from_slice(&(0x200 + index as u32).to_le_bytes());
        let f = Fixture::new(&body);
        let source = f.load(&mut f.store(), Default::default()).unwrap();
        let link = source.receipt().link.as_ref().unwrap();
        assert_eq!(link.target.status, "resolved");
        assert!(link.header_source_available);
        assert_eq!(link.source.as_ref().unwrap().header.kind, **kind);
        assert_eq!(
            link.source.as_ref().unwrap().header.offset,
            138 + 31 * index as u64
        );
        assert_eq!(source.receipt().usage.read_bytes, 72); // All target ZZZZ bodies lack placement NAME/DATA.
    }
    for kind in [b"REFR", b"ACHR", b"ACRE"] {
        let mut f = Fixture::new(&cell());
        f.patch(&[b"Base.esm"], &record(kind, 0x100, 0, &cell()));
        assert_eq!(
            f.load(&mut f.store(), Default::default())
                .unwrap()
                .receipt()
                .placed
                .header
                .kind,
            *kind
        );
    }
}
#[test]
fn absent_color_only_null_unavailable_and_reserved_runtime_player_keep_source_status() {
    let core = [field(b"NAME", &[0, 5, 0, 0]), field(b"DATA", &[0; 24])].concat();
    let f = Fixture::new(&core);
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    assert!(source.receipt().link.is_none());
    assert!(source.receipt().color.is_none());
    let f = Fixture::new(&[core, field(b"XCLP", &[1, 2, 3, 4, 5, 6, 7, 8])].concat());
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    assert!(source.receipt().link.is_none());
    assert!(source.receipt().color.is_some());
    for (raw, kind, flags, status) in [
        (0_u32, b"PGRE", 0, "null"),
        (0x999, b"PGRE", 0, "missing"),
        (0x200, b"PGRE", plugin::DELETED, "deleted"),
        (0x200, b"STAT", 0, "wrong-record-kind"),
    ] {
        let mut body = cell();
        body[38..42].copy_from_slice(&raw.to_le_bytes());
        let mut f = Fixture::new(&body);
        f.patch(&[b"Base.esm"], &record(kind, 0x200, flags, &[1, 2, 3]));
        let source = f.load(&mut f.store(), Default::default()).unwrap();
        let link = source.receipt().link.as_ref().unwrap();
        assert_eq!(link.target.status, status);
        assert!(!link.header_source_available);
        assert_eq!(
            link.source.is_some(),
            ["deleted", "wrong-record-kind"].contains(&status)
        );
    }
    let mut body = cell();
    body[38..42].copy_from_slice(&0x14_u32.to_le_bytes());
    let mut f = Fixture::new(&body);
    fs::rename(
        f.root.path().join("Data/Base.esm"),
        f.root.path().join("Data/FalloutNV.esm"),
    )
    .unwrap();
    f.names = vec!["FalloutNV.esm".into()];
    let root = FormKey {
        origin_plugin: "falloutnv.esm".into(),
        ..key(0x100)
    };
    let source = PlacedLinkedSources::load(&mut f.store(), &root, Default::default()).unwrap();
    let link = source.receipt().link.as_ref().unwrap();
    assert_eq!(link.target.status, "runtime-player-binding-unimplemented");
    assert_eq!(
        link.target.key,
        Some(FormKey {
            local_id: 0x14,
            ..root
        })
    );
    assert!(link.source.is_none());
    assert!(!link.header_source_available);
}
#[test]
fn cross_master_winning_placement_and_target_override_retain_original_identity() {
    let mut f = Fixture::new(&cell());
    fs::write(f.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    f.names.push("Other.esm".into());
    let mut body = cell();
    body[41] = 1;
    f.patch(
        &[b"Other.esm", b"Base.esm"],
        &[
            record(b"ACHR", 0x01000100, 0, &body),
            record(b"PBEA", 0x01000200, 0, &[1, 2, 3]),
        ]
        .concat(),
    );
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let r = source.receipt();
    let link = r.link.as_ref().unwrap();
    assert_eq!(r.placed.source_ordinal, 2);
    assert_eq!(r.placed.header.offset, 101);
    assert_eq!(link.raw, 0x01000200);
    assert_eq!(link.target.key, Some(key(0x200)));
    assert_eq!(link.source.as_ref().unwrap().source_ordinal, 2);
    assert_eq!(link.source.as_ref().unwrap().header.offset, 197);
    assert_eq!(link.source.as_ref().unwrap().header.kind, *b"PBEA");
    assert_eq!(r.usage.read_bytes, 72);
}
#[test]
fn duplicates_bad_width_truncated_tainted_or_invalid_roots_refuse_and_fields_preflight_before_decoder()
 {
    for (kind, width) in [(b"XLKR", 4), (b"XCLP", 8)] {
        let f = Fixture::new(&[cell(), field(kind, &vec![0; width])].concat());
        assert!(f.load(&mut f.store(), Default::default()).is_err());
        for bad in [0, 1, 2, 3, 5, 7, 9, 12] {
            if bad == width {
                continue;
            }
            let f = Fixture::new(
                &[
                    field(b"NAME", &[0, 5, 0, 0]),
                    field(b"DATA", &[0; 24]),
                    field(kind, &vec![0; bad]),
                ]
                .concat(),
            );
            assert!(f.load(&mut f.store(), Default::default()).is_err());
        }
    }
    let f = Fixture::new(&[cell(), b"XLKR\x04\0\0".to_vec()].concat());
    assert!(
        f.try_store(false)
            .and_then(|mut s| f.load(&mut s, Default::default()))
            .is_err()
    );
    let f = Fixture::new(&field(b"ZZZZ", &[1]));
    let e = f
        .load(
            &mut f.store(),
            Limits {
                fields: 0,
                ..Default::default()
            },
        )
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("physical fields budget"), "{e}"); // Budget fails before core decoder missing NAME.
    let mut f = Fixture::new(&cell());
    f.patch(&[b"Missing.esm"], &record(b"REFR", 0x100, 0, &cell()));
    assert!(f.try_store(false).is_err());
    let mut f = Fixture::new(&cell());
    f.patch(
        &[b"Base.esm"],
        &record(b"REFR", 0x100, plugin::DELETED, &[]),
    );
    assert!(f.load(&mut f.store(), Default::default()).is_err());
    let f = Fixture::new(&cell());
    let mut store = f.store();
    for root in [
        key(0x999),
        key(0x200),
        FormKey {
            profile: ProfileId::Fo3Original,
            ..key(0x100)
        },
    ] {
        assert!(PlacedLinkedSources::load(&mut store, &root, Default::default()).is_err());
    }
}
#[test]
fn extended_link_and_compressed_root_preserve_physical_and_visitor_ordinals() {
    let mut extended = cell()[..32].to_vec();
    extended.extend(field(b"XXXX", &4_u32.to_le_bytes()));
    extended.extend(b"XLKR\0\0");
    extended.extend([0, 2, 0, 0]);
    extended.extend(&cell()[42..]);
    let f = Fixture::new(&extended);
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let r = source.receipt();
    let link = r.link.as_ref().unwrap();
    assert_eq!(r.usage.fields, 6);
    assert_eq!(link.field.physical_field_ordinal, 4);
    assert_eq!(link.field.logical_field_ordinal, 3);
    assert_eq!(link.field.site.decoded_header_offset, 42);
    assert_eq!(link.field.site.span.decoded_offset, 48);
    assert_eq!(link.field.decoded_framing_offset, 32);
    assert_eq!(link.field.framing.len(), 20);
    assert_eq!(link.field.physical_framing_offset, Some(98));
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&extended).unwrap();
    let stored = [
        (extended.len() as u32).to_le_bytes().as_slice(),
        &encoder.finish().unwrap(),
    ]
    .concat();
    let mut f = Fixture::new(&cell());
    f.patch(
        &[b"Base.esm"],
        &record(b"REFR", 0x100, plugin::COMPRESSED, &stored),
    );
    let source = f.load(&mut f.store(), Default::default()).unwrap();
    let r = source.receipt();
    let field = &r.link.as_ref().unwrap().field;
    assert!(field.physical_framing_offset.is_none());
    assert!(
        r.color
            .as_ref()
            .unwrap()
            .field
            .physical_framing_offset
            .is_none()
    );
    assert_eq!(field.stored_body_offset, 95);
    assert_eq!(field.stored_body_bytes, stored.len() as u32);
    assert_eq!(field.site.decoded_header_offset, 42);
}
#[test]
fn all_eight_exact_allowances_and_one_under_refusal_leave_retry_identical() {
    let f = Fixture::new(&cell());
    let mut store = f.store();
    let source = f.load(&mut store, Default::default()).unwrap();
    let u = &source.receipt().usage;
    let exact = Limits {
        sources: u.sources,
        records: u.records,
        fields: u.fields,
        links: u.links,
        record_bytes: 72,
        read_bytes: u.read_bytes,
        raw_bytes: u.raw_bytes,
        metadata_bytes: u.metadata_bytes,
    };
    let identity = source.identity().to_owned();
    assert_eq!(f.load(&mut store, exact).unwrap().identity(), identity);
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
        assert!(f.load(&mut store, limit).is_err());
    }
    assert_eq!(f.load(&mut store, exact).unwrap().identity(), identity);
    assert!(
        f.load(
            &mut store,
            Limits {
                links: 2,
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
fn recovered_cell_checksum_body_cannot_construct_linked_authority() {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&cell()).unwrap();
    let mut stream = encoder.finish().unwrap();
    *stream.last_mut().unwrap() ^= 1;
    let stored = [72_u32.to_le_bytes().as_slice(), &stream].concat();
    let mut fixture = Fixture::new(&cell());
    fixture.patch(
        &[b"Base.esm"],
        &record(b"REFR", 0x100, plugin::COMPRESSED, &stored),
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
