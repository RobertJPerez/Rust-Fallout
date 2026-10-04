//! Independent CELL literals and header-only target witnesses.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    world::ownership::{CellOwnershipSources, Limits},
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
        field(b"XRNK", &[249, 255, 255, 255]),
        field(b"ZZZZ", &[91, 92]),
        field(b"XOWN", &[0, 2, 0, 0]),
    ]
    .concat()
}
const TARGETS: [&[u8; 4]; 2] = [b"FACT", b"NPC_"];
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
    ) -> fallout_data::Result<CellOwnershipSources> {
        CellOwnershipSources::load(store, &key(0x100), limits)
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
fn fact_npc_and_signed_extreme_rank_literals_retain_exact_source_words_and_spans() {
    for (word, value) in [
        ([0, 0, 0, 128], i32::MIN),
        ([255, 255, 255, 127], i32::MAX),
        ([249, 255, 255, 255], -7),
        ([0, 0, 0, 0], 0),
    ] {
        for (owner_word, id, kind, header_offset) in [
            ([0, 2, 0, 0], 0x200_u32, b"FACT", 101),
            ([1, 2, 0, 0], 0x201, b"NPC_", 132),
        ] {
            let mut body = cell();
            body[13..17].copy_from_slice(&word);
            body[31..35].copy_from_slice(&owner_word);
            let fixture = Fixture::new(&body);
            let sources = fixture
                .load(&mut fixture.store(), Default::default())
                .unwrap();
            let r = sources.receipt();
            assert_eq!(r.cell.header.offset, 42);
            assert_eq!(r.cell_flags.value, 1);
            assert_eq!(
                (
                    r.usage.records,
                    r.usage.fields,
                    r.usage.links,
                    r.usage.read_bytes,
                    r.usage.raw_bytes
                ),
                (2, 4, 1, 35, 20)
            );
            let owner = r.owner.as_ref().unwrap();
            assert_eq!(owner.raw, id);
            assert_eq!(owner.target.key, Some(key(id)));
            assert_eq!(owner.target.status, "resolved");
            assert_eq!(owner.target.expected_kinds, vec!["NPC_", "FACT"]);
            let source = owner.source.as_ref().unwrap();
            assert_eq!(source.header.kind, *kind);
            assert_eq!(source.header.offset, header_offset);
            assert_eq!(source.source_ordinal, 0);
            assert!(owner.header_source_available);
            assert_eq!(
                owner.ownership_status,
                "unknown; target body and ownership not evaluated"
            );
            assert_eq!(owner.field.physical_field_ordinal, 3);
            assert_eq!(owner.field.logical_field_ordinal, 3);
            assert_eq!(owner.field.site.decoded_header_offset, 25);
            assert_eq!(owner.field.site.span.decoded_offset, 31);
            assert_eq!(owner.field.physical_framing_offset, Some(91));
            assert_eq!(owner.field.framing, field(b"XOWN", &owner_word));
            let rank = r.rank.as_ref().unwrap();
            assert_eq!(rank.raw, u32::from_le_bytes(word));
            assert_eq!(rank.value, value);
            assert_eq!(rank.field.physical_field_ordinal, 1);
            assert_eq!(rank.field.logical_field_ordinal, 1);
            assert_eq!(rank.field.site.decoded_header_offset, 7);
            assert_eq!(rank.field.site.span.decoded_offset, 13);
            assert_eq!(rank.field.physical_framing_offset, Some(73));
            assert_eq!(rank.field.framing, field(b"XRNK", &word));
            assert_eq!(
                rank.applicability_status,
                "unknown; actor/faction rank and ownership not evaluated"
            );
            assert!(!r.runtime_ready);
        }
    }
}
#[test]
fn absent_rank_without_owner_null_missing_deleted_wrong_kind_stay_unknown_not_unowned() {
    let fixture = Fixture::new(&field(b"DATA", &[1]));
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    assert!(sources.receipt().owner.is_none());
    assert!(sources.receipt().rank.is_none());
    let fixture = Fixture::new(&[field(b"DATA", &[1]), field(b"XRNK", &[0, 0, 0, 128])].concat());
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    assert!(sources.receipt().owner.is_none());
    assert_eq!(sources.receipt().rank.as_ref().unwrap().value, i32::MIN);
    for (raw, kind, flags, status) in [
        (0_u32, b"FACT", 0, "null"),
        (0x999, b"FACT", 0, "missing"),
        (0x200, b"FACT", plugin::DELETED, "deleted"),
        (0x200, b"MUSC", 0, "wrong-record-kind"),
    ] {
        let mut fixture =
            Fixture::new(&[field(b"DATA", &[1]), field(b"XOWN", &raw.to_le_bytes())].concat());
        fixture.patch(&[b"Base.esm"], &record(kind, 0x200, flags, &[1, 2, 3]));
        let sources = fixture
            .load(&mut fixture.store(), Default::default())
            .unwrap();
        let owner = sources.receipt().owner.as_ref().unwrap();
        assert_eq!(owner.target.status, status);
        assert!(!owner.header_source_available);
        assert_eq!(
            owner.source.is_some(),
            ["deleted", "wrong-record-kind"].contains(&status)
        );
        assert!(sources.receipt().rank.is_none());
        assert_eq!(sources.receipt().usage.read_bytes, 17);
    }
}
#[test]
fn two_master_cell_and_owner_override_retain_canonical_identity_without_reading_target_behavior() {
    let mut fixture = Fixture::new(&cell());
    fs::write(fixture.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    fixture.names.push("Other.esm".into());
    let mut body = cell();
    body[34] = 1;
    fixture.patch(
        &[b"Other.esm", b"Base.esm"],
        &[
            record(b"CELL", 0x01000100, 0, &body),
            record(b"FACT", 0x01000200, 0, &[1, 2, 3]),
        ]
        .concat(),
    );
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    let owner = r.owner.as_ref().unwrap();
    assert_eq!(r.cell.source_ordinal, 2);
    assert_eq!(r.cell.header.offset, 101);
    assert_eq!(owner.raw, 0x01000200);
    assert_eq!(owner.target.key, Some(key(0x200)));
    assert_eq!(owner.source.as_ref().unwrap().source_ordinal, 2);
    assert_eq!(owner.source.as_ref().unwrap().header.offset, 160);
    assert_eq!(r.usage.read_bytes, 35);
    assert_eq!(r.rank.as_ref().unwrap().value, -7);
}
#[test]
fn duplicates_short_fo4_wide_xglb_taint_and_invalid_roots_never_create_nv_ownership() {
    for kind in [b"XOWN", b"XRNK"] {
        let fixture = Fixture::new(&[cell(), field(kind, &[0; 4])].concat());
        assert!(
            fixture
                .load(&mut fixture.store(), Default::default())
                .is_err()
        );
        for width in [0, 1, 2, 3, 5, 8, 12] {
            let fixture =
                Fixture::new(&[field(b"DATA", &[1]), field(kind, &vec![0; width])].concat());
            assert!(
                fixture
                    .load(&mut fixture.store(), Default::default())
                    .is_err()
            );
        }
    }
    for width in [0, 4, 8] {
        let fixture = Fixture::new(&[cell(), field(b"XGLB", &vec![0; width])].concat());
        let e = fixture
            .load(&mut fixture.store(), Default::default())
            .err()
            .unwrap()
            .to_string();
        assert!(
            e.contains("XGLB is raw unknown/unsupported for FNV ownership at decoded header 35"),
            "{e}"
        );
    }
    let fixture = Fixture::new(&[field(b"DATA", &[1]), b"XOWN\x04\0\0".to_vec()].concat());
    assert!(
        fixture
            .try_store(false)
            .and_then(|mut s| fixture.load(&mut s, Default::default()))
            .is_err()
    );
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
        assert!(CellOwnershipSources::load(&mut store, &root, Default::default()).is_err());
    }
}
#[test]
fn extended_rank_and_compressed_cell_keep_decoded_sites_separate_from_physical_offsets() {
    let mut extended = field(b"DATA", &[1]);
    extended.extend(field(b"XXXX", &4_u32.to_le_bytes()));
    extended.extend(b"XRNK\0\0");
    extended.extend([249, 255, 255, 255]);
    extended.extend(&cell()[17..]);
    let fixture = Fixture::new(&extended);
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let r = sources.receipt();
    let rank = r.rank.as_ref().unwrap();
    assert_eq!(rank.field.physical_field_ordinal, 2);
    assert_eq!(rank.field.logical_field_ordinal, 1);
    assert_eq!(rank.field.site.decoded_header_offset, 17);
    assert_eq!(rank.field.site.span.decoded_offset, 23);
    assert_eq!(rank.field.decoded_framing_offset, 7);
    assert_eq!(rank.field.framing.len(), 20);
    assert_eq!(rank.field.physical_framing_offset, Some(73));
    assert_eq!(r.usage.fields, 5);
    let owner = r.owner.as_ref().unwrap();
    assert_eq!(owner.field.physical_field_ordinal, 4);
    assert_eq!(owner.field.logical_field_ordinal, 3);
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
    let r = sources.receipt();
    assert!(
        r.owner
            .as_ref()
            .unwrap()
            .field
            .physical_framing_offset
            .is_none()
    );
    let rank = r.rank.as_ref().unwrap();
    assert!(rank.field.physical_framing_offset.is_none());
    assert_eq!(rank.field.stored_body_offset, 95);
    assert_eq!(rank.field.stored_body_bytes, stored.len() as u32);
    assert_eq!(rank.field.site.decoded_header_offset, 17);
}
#[test]
fn all_eight_exact_lowerable_allowances_and_one_under_refusal_leave_retry_identical() {
    let fixture = Fixture::new(&cell());
    let mut store = fixture.store();
    let source = fixture.load(&mut store, Default::default()).unwrap();
    let u = &source.receipt().usage;
    let exact = Limits {
        sources: u.sources,
        records: u.records,
        fields: u.fields,
        links: u.links,
        record_bytes: 35,
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
fn recovered_cell_checksum_body_cannot_construct_ownership_authority() {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&cell()).unwrap();
    let mut stream = encoder.finish().unwrap();
    *stream.last_mut().unwrap() ^= 1;
    let stored = [35_u32.to_le_bytes().as_slice(), &stream].concat();
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
