use fallout_data::{
    actors::packages::{
        self,
        destinations::{self, Alternative, Limits},
    },
    identity::{FormKey, ProfileId},
    inventory, plugin,
    store::RecordStore,
};
use std::{fs, io::Write, path::Path};

fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn field(kind: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(raw.len() as u16).to_le_bytes(), raw].concat()
}
fn disk(kind: &[u8; 4], id: u32, flags: u32, raw: &[u8]) -> Vec<u8> {
    let body = if flags & plugin::COMPRESSED != 0 {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(raw).unwrap();
        [
            &(raw.len() as u32).to_le_bytes()[..],
            &encoder.finish().unwrap(),
        ]
        .concat()
    } else {
        raw.to_vec()
    };
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0xA5, 0x5A],
        &body,
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
    disk(b"TES4", 0, 0, &body)
}
fn operand(
    kind: &[u8; 4],
    discriminant: i32,
    union: u32,
    scalar: i32,
    tail: Option<u32>,
) -> Vec<u8> {
    let mut raw = [
        discriminant.to_le_bytes().as_slice(),
        &union.to_le_bytes(),
        &scalar.to_le_bytes(),
    ]
    .concat();
    if let Some(bits) = tail {
        raw.extend(bits.to_le_bytes());
    }
    field(kind, &raw)
}
fn body(extra: Vec<u8>) -> Vec<u8> {
    [
        field(b"PKDT", &[0, 0, 0, 0, 6, 0, 0, 0]),
        field(b"PSDT", &[255, 255, 255, 255, 0, 0, 0, 0]),
        extra,
    ]
    .concat()
}
fn valid() -> Vec<u8> {
    [
        operand(b"PLDT", 0, 0x300, i32::MIN, None),
        operand(b"PLD2", 1, 0x301, -100, None),
        operand(b"PTDT", 0, 0x300, 7, Some(0x7FC0_0031)),
        operand(b"PTD2", 1, 0x302, -9, None),
    ]
    .concat()
}
fn cases() -> Vec<(u32, u32, Vec<u8>)> {
    vec![
        (0x100, 0, valid()),
        (
            0x101,
            0,
            [
                operand(b"PLDT", 2, 0x300, -1, None),
                operand(b"PLD2", 5, 28, 1, None),
                operand(b"PTDT", 2, u32::MAX, -3, Some(0x80000000)),
                operand(b"PTD2", 3, 0x300, 0, None),
            ]
            .concat(),
        ),
        (
            0x102,
            0,
            [
                operand(b"PLDT", 0, 0x300, 1, None),
                operand(b"PLDT", 1, 0x301, 2, None),
                operand(b"PTDT", 1, 0x304, 3, None),
                operand(b"PTDT", 1, 0x304, 4, None),
            ]
            .concat(),
        ),
        (
            0x103,
            0,
            [
                operand(b"PLDT", i32::MIN, 0x300, -1, None),
                operand(b"PTDT", i32::MAX, 0x300, -2, Some(0xFFFFFFFF)),
            ]
            .concat(),
        ),
        (
            0x104,
            0,
            [
                operand(b"PLDT", 0, 0, 0, None),
                operand(b"PTD2", 1, 0, -1, None),
            ]
            .concat(),
        ),
        (
            0x105,
            0,
            [
                operand(b"PLD2", 1, 0x999, 0, None),
                operand(b"PTDT", 0, 0x14, 1, None),
            ]
            .concat(),
        ),
        (
            0x106,
            0,
            [
                operand(b"PLDT", 1, 0x302, 0, None),
                operand(b"PTDT", 0, 0x303, 0, None),
            ]
            .concat(),
        ),
        (
            0x107,
            0,
            [
                operand(b"PLDT", 0, 0x306, 0, None),
                operand(b"PTD2", 1, 0x307, 0, None),
            ]
            .concat(),
        ),
        (
            0x108,
            0,
            [
                operand(b"PTDT", 1, 0x304, 0, None),
                operand(b"PTD2", 1, 0x304, 0, None),
            ]
            .concat(),
        ),
        (
            0x109,
            0,
            [
                operand(b"PLDT", 4, 0x303, 0, None),
                operand(b"PTDT", 1, 0x303, 0, None),
            ]
            .concat(),
        ),
        (
            0x10A,
            0,
            [
                field(b"PLDT", &[0, 1, 2, 3, 4, 5, 6]),
                field(b"PTDT", &[0; 20]),
            ]
            .concat(),
        ),
        (0x10B, 0, Vec::new()),
        (0x10C, plugin::COMPRESSED, valid()),
        (
            0x10D,
            0,
            [
                field(b"UNKN", b"opaque"),
                field(b"XXXX", &12u32.to_le_bytes()),
                b"PLDT\0\0".to_vec(),
                [
                    0i32.to_le_bytes().as_slice(),
                    &0x300u32.to_le_bytes(),
                    &i32::MAX.to_le_bytes(),
                ]
                .concat(),
            ]
            .concat(),
        ),
        (
            0x10E,
            0,
            [
                operand(b"PLDT", 3, 0x300, -1, None),
                operand(b"PLD2", 6, 0x301, -2, None),
                operand(b"PTDT", 3, 0x300, -3, None),
                operand(b"PLD2", 7, 0x300, -4, None),
            ]
            .concat(),
        ),
        (
            0x10F,
            0,
            [
                operand(b"PLDT", 4, 0x302, -1, None),
                operand(b"PTD2", 2, 0, 0, Some(0x7F800000)),
            ]
            .concat(),
        ),
    ]
}
fn fixture(path: &Path, patched: bool) -> RecordStore {
    let mut bytes = header(&[]);
    // Target contents remain opaque, with valid common subrecord framing.
    // Requests only bind their headers and never interpret these bodies.
    for (kind, id, flags) in [
        (b"REFR", 0x300, 0),
        (b"CELL", 0x301, 0),
        (b"ARMO", 0x302, 0),
        (b"FACT", 0x303, 0),
        (b"IDLM", 0x304, 0),
        (b"FLST", 0x305, 0),
        (b"REFR", 0x306, plugin::DELETED),
        (b"LVLI", 0x307, 0),
    ] {
        bytes.extend(disk(
            kind,
            id,
            flags,
            &field(b"UNKN", b"opaque target bytes"),
        ));
    }
    for (id, flags, extra) in cases() {
        bytes.extend(disk(b"PACK", id, flags, &body(extra)));
    }
    bytes.extend(disk(
        b"PACK",
        0x110,
        plugin::DELETED,
        &field(b"UNKN", b"opaque deleted bytes"),
    ));
    fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
    let mut names = vec!["FalloutNV.esm".to_string()];
    if patched {
        let extra = [
            operand(b"PLDT", 1, 0x301, -11, None),
            operand(b"PTDT", 1, 0x02000400, 12, Some(0x80000000)),
        ]
        .concat();
        fs::write(
            path.join("Patch.esp"),
            [
                header(&["FalloutNV.esm"]),
                disk(b"ARMO", 0x02000400, 0, &field(b"UNKN", b"private target")),
                disk(b"PACK", 0x100, 0, &body(extra)),
            ]
            .concat(),
        )
        .unwrap();
        names.push("Patch.esp".into());
    }
    RecordStore::open_nv_headers(path, &names, Default::default()).unwrap()
}

#[test]
fn explicit_operands_preserve_signed_values_optional_float_bits_exact_origins_and_borrowed_package()
{
    let dir = tempfile::tempdir().unwrap();
    let mut store = fixture(dir.path(), false);
    let catalogue = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    let manifest =
        destinations::request(&mut store, &catalogue, &key(0x100), Limits::default()).unwrap();
    assert!(std::ptr::eq(
        manifest.package,
        catalogue.get(&key(0x100)).unwrap()
    ));
    assert_eq!(manifest.field_visits, 6);
    assert_eq!(manifest.operand_bytes, 52);
    assert_eq!(
        manifest
            .operands
            .iter()
            .map(|o| o.field_decoded_offset)
            .collect::<Vec<_>>(),
        [28, 46, 64, 86]
    );
    assert_eq!(
        manifest
            .operands
            .iter()
            .map(|o| o.signed_scalar.unwrap())
            .collect::<Vec<_>>(),
        [i32::MIN, -100, 7, -9]
    );
    assert_eq!(manifest.operands[2].unknown_float_bits, Some(0x7FC00031));
    assert_eq!(manifest.operands[3].unknown_float_bits, None);
    assert_eq!(
        manifest
            .operands
            .iter()
            .map(|o| o.alternative)
            .collect::<Vec<_>>(),
        [
            Alternative::Reference,
            Alternative::Cell,
            Alternative::Reference,
            Alternative::ObjectId
        ]
    );
    for (o, id, kind) in manifest
        .operands
        .iter()
        .zip([0x300, 0x301, 0x300, 0x302])
        .zip([b"REFR", b"CELL", b"REFR", b"ARMO"])
        .map(|((o, id), kind)| (o, id, kind))
    {
        assert!(o.binding_admitted);
        assert_eq!(o.binding.as_ref().unwrap().key, Some(key(id)));
        assert_eq!(
            &o.binding.as_ref().unwrap().target.as_ref().unwrap().kind,
            kind
        );
    }
    assert!(manifest.source_layouts_supported);
    assert!(!manifest.path_target_selected && !manifest.execution_supported);
}
#[test]
fn non_form_union_words_never_bind_unknown_unused_object_types_or_future_layouts() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = fixture(dir.path(), false);
    let catalogue = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    for id in [0x101, 0x103, 0x10A, 0x10E] {
        let m = destinations::request(&mut store, &catalogue, &key(id), Limits::default()).unwrap();
        for o in &m.operands {
            assert!(o.binding.is_none());
            assert!(!o.binding_admitted);
            if o.object_type_known != Some(true) {
                assert!(!o.findings.is_empty());
            }
        }
        if id != 0x10E {
            assert!(!m.source_layouts_supported);
        }
    }
    let m = destinations::request(&mut store, &catalogue, &key(0x101), Limits::default()).unwrap();
    assert_eq!(m.operands[1].object_type_known, Some(true));
    assert_eq!(m.operands[2].object_type_known, Some(false));
    let m = destinations::request(&mut store, &catalogue, &key(0x10A), Limits::default()).unwrap();
    assert_eq!(m.operands[0].raw_bytes, [0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(m.operands[0].discriminant, None);
}
#[test]
fn repeats_null_missing_deleted_wrong_kind_and_missing_operands_cannot_choose_a_target() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = fixture(dir.path(), false);
    let catalogue = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    for id in 0x102..=0x107 {
        let m = destinations::request(&mut store, &catalogue, &key(id), Limits::default()).unwrap();
        assert!(m.operands.iter().all(|o| !o.binding_admitted));
        assert!(!m.path_target_selected);
    }
    let m = destinations::request(&mut store, &catalogue, &key(0x102), Limits::default()).unwrap();
    assert!(m.operands.iter().all(|o| o.repeated));
    assert_eq!(m.operands.len(), 4);
    for (id, status) in [
        (0x104, inventory::Status::Null),
        (0x105, inventory::Status::Missing),
    ] {
        let m = destinations::request(&mut store, &catalogue, &key(id), Limits::default()).unwrap();
        assert!(
            m.operands
                .iter()
                .all(|o| o.binding.as_ref().unwrap().status == status)
        );
    }
    let m = destinations::request(&mut store, &catalogue, &key(0x10B), Limits::default()).unwrap();
    assert!(m.operands.is_empty());
    assert_eq!(m.issues, ["no_authored_destination_operands"]);
}
#[test]
fn schema_target_one_idlm_and_location_target_object_sets_remain_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = fixture(dir.path(), false);
    let catalogue = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    let m = destinations::request(&mut store, &catalogue, &key(0x108), Limits::default()).unwrap();
    assert!(m.operands[0].binding_admitted);
    assert_eq!(m.operands[1].schema_kind_allowed, Some(false));
    assert!(!m.operands[1].binding_admitted);
    let m = destinations::request(&mut store, &catalogue, &key(0x109), Limits::default()).unwrap();
    assert_eq!(m.operands[0].schema_kind_allowed, Some(false));
    assert!(m.operands[1].binding_admitted);
}
#[test]
fn override_master_binding_compressed_and_extended_fields_follow_exact_winners() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = fixture(dir.path(), true);
    let catalogue = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    let m = destinations::request(&mut store, &catalogue, &key(0x100), Limits::default()).unwrap();
    assert_eq!(m.package.source.plugin, "Patch.esp");
    assert_eq!(m.operands[0].signed_scalar, Some(-11));
    assert_eq!(
        m.operands[1]
            .binding
            .as_ref()
            .unwrap()
            .key
            .as_ref()
            .unwrap()
            .origin_plugin,
        "patch.esp"
    );
    assert_eq!(
        m.operands[1]
            .binding
            .as_ref()
            .unwrap()
            .key
            .as_ref()
            .unwrap()
            .local_id,
        0x400
    );
    let m = destinations::request(&mut store, &catalogue, &key(0x10C), Limits::default()).unwrap();
    assert_eq!(m.operands[0].signed_scalar, Some(i32::MIN));
    assert_eq!(
        m.operands[1].binding.as_ref().unwrap().key,
        Some(key(0x301))
    );
    assert_eq!(m.operands[2].unknown_float_bits, Some(0x7FC00031));
    assert_eq!(m.operand_bytes, 52);
    let m = destinations::request(&mut store, &catalogue, &key(0x10D), Limits::default()).unwrap();
    assert_eq!(m.operands[0].field_index, 3);
    assert_eq!(m.operands[0].field_decoded_offset, 50);
    assert_eq!(m.operands[0].signed_scalar, Some(i32::MAX));
}
#[test]
fn exact_budgets_and_foreign_deleted_wrong_kind_roots_refuse_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = fixture(dir.path(), false);
    let catalogue = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    let m = destinations::request(&mut store, &catalogue, &key(0x100), Limits::default()).unwrap();
    let bytes = serde_json::to_vec(&m).unwrap().len();
    let exact = Limits {
        max_sources: 1,
        max_field_visits: 6,
        max_operands: 4,
        max_operand_bytes: 52,
        max_projection_bytes: bytes,
    };
    assert!(destinations::request(&mut store, &catalogue, &key(0x100), exact).is_ok());
    for bad in [
        Limits {
            max_sources: 0,
            ..exact
        },
        Limits {
            max_field_visits: 5,
            ..exact
        },
        Limits {
            max_operands: 3,
            ..exact
        },
        Limits {
            max_operand_bytes: 51,
            ..exact
        },
        Limits {
            max_projection_bytes: bytes - 1,
            ..exact
        },
    ] {
        assert!(destinations::request(&mut store, &catalogue, &key(0x100), bad).is_err());
    }
    for id in [0x999, 0x302, 0x110] {
        assert!(
            destinations::request(&mut store, &catalogue, &key(id), Limits::default()).is_err()
        );
    }
    let other = tempfile::tempdir().unwrap();
    let mut changed = fixture(other.path(), true);
    assert!(
        destinations::request(&mut changed, &catalogue, &key(0x100), Limits::default()).is_err()
    );
}
#[test]
fn export_independent_destination_fixtures_when_requested() {
    let Ok(path) = std::env::var("FALLOUT_ACTOR_DESTINATION_EVIDENCE_DIR") else {
        return;
    };
    let root = Path::new(&path);
    for (name, patched) in [("base", false), ("override", true)] {
        let install = root.join(name);
        let data = install.join("Data");
        fs::create_dir_all(&data).unwrap();
        let mut store = fixture(&data, patched);
        let catalogue = packages::Catalogue::load(&mut store, Default::default()).unwrap();
        let names = if patched {
            vec!["FalloutNV.esm", "Patch.esp"]
        } else {
            vec!["FalloutNV.esm"]
        };
        fs::write(
            install.join("order.json"),
            serde_json::to_vec_pretty(&names).unwrap(),
        )
        .unwrap();
        for (id, _, _) in cases() {
            let m =
                destinations::request(&mut store, &catalogue, &key(id), Limits::default()).unwrap();
            fs::write(
                install.join(format!("host-{id:X}.json")),
                serde_json::to_vec_pretty(&m).unwrap(),
            )
            .unwrap();
        }
    }
}
