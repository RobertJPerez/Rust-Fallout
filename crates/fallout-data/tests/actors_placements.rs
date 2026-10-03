use fallout_data::{
    actors::placements::{Catalogue, Limits, Value},
    inventory::Status,
    plugin, record_metadata,
    store::RecordStore,
};
use std::{fs, io::Write, path::Path};
fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn link(kind: &[u8; 4], raw: u32) -> Vec<u8> {
    field(kind, &raw.to_le_bytes())
}
fn disk(kind: &[u8; 4], raw: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
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
        kind.as_slice(),
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
    disk(b"TES4", 0, 0, 15, &body)
}
fn core(base: u32, first: u32) -> Vec<u8> {
    let words = [first, 0x3f80_0000, 1, 0x3f80_0001, 0x7f7f_ffff, 0xff7f_ffff];
    [
        link(b"NAME", base),
        field(
            b"DATA",
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        field(b"XSCL", &1u32.to_le_bytes()),
    ]
    .concat()
}
fn targets() -> Vec<u8> {
    [
        (b"NPC_", 0x800),
        (b"CREA", 0x801),
        (b"ECZN", 0x900),
        (b"REFR", 0x901),
        (b"PACK", 0x902),
    ]
    .into_iter()
    .flat_map(|(kind, raw)| disk(kind, raw, 0, 15, &[]))
    .collect()
}
fn source(path: &Path, order: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn single(path: &Path, kind: &[u8; 4], version: u16, body: &[u8]) -> RecordStore {
    fs::write(
        path.join("FalloutNV.esm"),
        [header(&[]), targets(), disk(kind, 0x100, 0, version, body)].concat(),
    )
    .unwrap();
    source(path, &["FalloutNV.esm"])
}
fn group(label: u32, kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &label.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn children(cell: u32, body: &[u8]) -> Vec<u8> {
    group(cell, 6, &group(cell, 9, body))
}
fn cell(raw: u32, name: &[u8]) -> Vec<u8> {
    disk(
        b"CELL",
        raw,
        0,
        15,
        &[field(b"EDID", name), field(b"DATA", &[1])].concat(),
    )
}

#[test]
fn retained_world_core_keeps_signed_zero_subnormal_bits_scale_and_source_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        core(0x800, 0x8000_0000),
        link(b"XEZN", 0x900),
        link(b"XMRC", 0x901),
        field(b"XLCM", &i32::MIN.to_le_bytes()),
    ]
    .concat();
    let mut store = single(directory.path(), b"ACHR", 15, &body);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let (key, definition) = catalogue.iter().next().unwrap();
    assert!(std::ptr::eq(catalogue.get(key).unwrap(), definition));
    let output = definition.core.as_ref().unwrap();
    assert_eq!(output.base.value, 0x800);
    assert_eq!(output.base.decoded_offset, 0);
    assert_eq!(output.transform_decoded_offset, 10);
    assert_eq!(output.position_bits, [0x8000_0000, 0x3f80_0000, 1]);
    assert_eq!(
        output.rotation_bits,
        [0x3f80_0001, 0x7f7f_ffff, 0xff7f_ffff]
    );
    assert_eq!(output.scale.as_ref().unwrap().value, 1);
    assert_eq!(output.scale.as_ref().unwrap().decoded_offset, 40);
    assert_eq!(
        definition.placement().unwrap().transform.value.position[0].to_bits(),
        0x8000_0000
    );
    assert_eq!(definition.record().unwrap().payload, body);
    assert_eq!(definition.base.as_ref().unwrap().status, Status::Defined);
    assert_eq!(definition.base_schema_kind_allowed, Some(true));
    assert!(matches!(
        definition.fields[5].value,
        Value::LevelModifier { modifier: i32::MIN }
    ));
    assert_eq!(catalogue.counts().bindings, 3);
    assert_eq!(catalogue.counts().selected_extra_fields, 3);
    assert!(definition.findings.is_empty());
    assert_eq!(
        catalogue.winning_content_sha256(),
        record_metadata::inspect(&store)
            .unwrap()
            .winning_definitions_sha256
    );
    assert_eq!(
        catalogue.sources()[0].source_sha256,
        definition.source.sha256
    );
}

#[test]
fn ordered_extra_occurrences_preserve_target_states_and_duplicate_signed_modifiers() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        core(0x902, 0),
        link(b"XEZN", 0x999),
        link(b"XMRC", 0x902),
        link(b"XEZN", 0x900),
        link(b"XMRC", 0),
        field(b"XLCM", &i32::MAX.to_le_bytes()),
        field(b"XLCM", &(-1i32).to_le_bytes()),
    ]
    .concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), targets(), disk(b"ACHR", 0x100, 0, 15, &body)].concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Patch.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(b"ECZN", 0x900, plugin::DELETED, 16, b"unread"),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm", "Patch.esm"]);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(definition.base_schema_kind_allowed, Some(false));
    assert!(
        matches!(&definition.fields[3].value,Value::EncounterZone{zone,schema_kind_allowed:None} if zone.status==Status::Missing)
    );
    assert!(
        matches!(&definition.fields[4].value,Value::MerchantContainer{container,schema_kind_allowed:Some(false)} if container.status==Status::Defined)
    );
    assert!(
        matches!(&definition.fields[5].value,Value::EncounterZone{zone,schema_kind_allowed:Some(true)} if zone.status==Status::Deleted)
    );
    assert!(
        matches!(&definition.fields[6].value,Value::MerchantContainer{container,schema_kind_allowed:None} if container.status==Status::Null)
    );
    assert!(matches!(
        definition.fields[7].value,
        Value::LevelModifier { modifier: i32::MAX }
    ));
    assert!(matches!(
        definition.fields[8].value,
        Value::LevelModifier { modifier: -1 }
    ));
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        vec![
            "placement_target_wrong_kind",
            "placement_target_missing",
            "placement_target_wrong_kind",
            "multiple_placed_encounter_zone_fields",
            "placement_target_deleted",
            "multiple_placed_merchant_fields",
            "multiple_placed_level_modifier_fields"
        ]
    );
    assert!(
        definition
            .findings
            .windows(2)
            .all(|pair| pair[0].field_decoded_offset <= pair[1].field_decoded_offset)
    );
}

#[test]
fn missing_optional_extras_remain_absent_and_extended_unknown_fields_keep_order_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let extended = [
        field(b"XXXX", &6u32.to_le_bytes()),
        b"UNKN\0\0opaque".to_vec(),
    ]
    .concat();
    let body = [core(0x801, 0), extended].concat();
    let mut store = single(directory.path(), b"ACRE", 11, &body);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(catalogue.counts().selected_extra_fields, 0);
    assert_eq!(catalogue.counts().bindings, 1);
    assert_eq!(definition.fields[3].kind, *b"UNKN");
    assert_eq!(definition.fields[3].decoded_offset, 60);
    assert_eq!(definition.fields[3].bytes, 6);
    assert!(matches!(definition.fields[3].value, Value::Opaque));
    assert_eq!(definition.record().unwrap().payload, body);
    assert!(definition.findings.is_empty());
}

#[test]
fn observed_kind_versions_and_existing_world_constraints_have_explicit_failures() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, versions) in [(b"ACHR", vec![15]), (b"ACRE", vec![9, 11, 15])] {
        for version in versions {
            let base = if kind == b"ACHR" { 0x800 } else { 0x801 };
            let mut store = single(directory.path(), kind, version, &core(base, 0));
            assert!(Catalogue::load(&mut store, Limits::default()).is_ok());
        }
    }
    for (kind, version) in [(b"ACHR", 14), (b"ACHR", 9), (b"ACRE", 14), (b"ACRE", 16)] {
        let mut store = single(directory.path(), kind, version, &core(0x800, 0));
        assert!(Catalogue::load(&mut store, Limits::default()).is_err());
    }
    let mut cases = vec![
        field(b"DATA", &[0; 24]),
        link(b"NAME", 0x800),
        core(0x800, 0x7fc0_1234),
        [core(0x800, 0), link(b"NAME", 0x800)].concat(),
        [
            link(b"NAME", 0x800),
            field(b"DATA", &[0; 24]),
            field(b"XSCL", &0u32.to_le_bytes()),
        ]
        .concat(),
        b"DAT".to_vec(),
        field(b"XXXX", &4u32.to_le_bytes()),
    ];
    for kind in [b"XEZN", b"XMRC", b"XLCM", b"XLKR"] {
        for length in [3, 5] {
            cases.push([core(0x800, 0), field(kind, &vec![0; length])].concat());
        }
    }
    for body in cases {
        let mut store = single(directory.path(), b"ACHR", 15, &body);
        assert!(Catalogue::load(&mut store, Limits::default()).is_err());
    }
}

#[test]
fn linked_references_keep_all_seven_domains_target_states_and_physical_occurrences() {
    let directory = tempfile::tempdir().unwrap();
    let raw_targets = [
        0x911, 0x910, 0x100, 0x912, 0x913, 0x914, 0x915, 0, 0x999, 0x902, 0x916,
    ];
    let body = [
        core(0x800, 0),
        raw_targets
            .iter()
            .flat_map(|raw| link(b"XLKR", *raw))
            .collect(),
    ]
    .concat();
    let generic_targets: Vec<_> = [
        (b"REFR", 0x911),
        (b"PGRE", 0x912),
        (b"PMIS", 0x913),
        (b"PBEA", 0x914),
        (b"PLYR", 0x915),
        (b"REFR", 0x916),
    ]
    .into_iter()
    .flat_map(|(kind, raw)| disk(kind, raw, 0, 15, &[]))
    .collect();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            targets(),
            generic_targets,
            disk(b"ACRE", 0x910, 0, 9, &core(0x801, 0)),
            disk(b"ACHR", 0x100, 0, 15, &body),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Patch.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(b"REFR", 0x916, plugin::DELETED, 16, b"unread"),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm", "Patch.esm"]);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue
        .iter()
        .find(|(key, _)| key.local_id == 0x100)
        .unwrap()
        .1;
    let expected_kinds = [
        *b"REFR", *b"ACRE", *b"ACHR", *b"PGRE", *b"PMIS", *b"PBEA", *b"PLYR",
    ];
    for (index, output) in definition.fields[3..].iter().enumerate() {
        let Value::LinkedReference {
            reference,
            schema_kind_allowed,
        } = &output.value
        else {
            panic!("linked reference")
        };
        assert_eq!(output.kind, *b"XLKR");
        assert_eq!(output.decoded_offset, 50 + index as u32 * 10);
        assert_eq!(reference.raw_form, raw_targets[index]);
        if index < 7 {
            assert_eq!(reference.status, Status::Defined);
            assert_eq!(
                reference.target.as_ref().unwrap().kind,
                expected_kinds[index]
            );
            assert_eq!(*schema_kind_allowed, Some(true));
        } else {
            assert_eq!(
                reference.status,
                [
                    Status::Null,
                    Status::Missing,
                    Status::Defined,
                    Status::Deleted
                ][index - 7]
            );
            assert_eq!(
                *schema_kind_allowed,
                [None, None, Some(false), Some(true)][index - 7]
            );
        }
    }
    assert_eq!(definition.findings.len(), 13);
    assert_eq!(
        definition
            .findings
            .iter()
            .filter(|finding| finding.code == "multiple_placed_linked_reference_fields")
            .count(),
        10
    );
    assert_eq!(
        definition
            .findings
            .iter()
            .rev()
            .take(4)
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        [
            "placement_target_deleted",
            "multiple_placed_linked_reference_fields",
            "placement_target_wrong_kind",
            "multiple_placed_linked_reference_fields"
        ]
    );
    assert!(
        definition
            .findings
            .windows(2)
            .all(|pair| pair[0].field_decoded_offset <= pair[1].field_decoded_offset)
    );
    assert_eq!(catalogue.counts().selected_extra_fields, 11);
    assert_eq!(catalogue.counts().bindings, 13);
    assert_eq!(definition.record().unwrap().payload, body);
    assert!(
        Catalogue::load(
            &mut store,
            Limits {
                max_bindings: 12,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(
        Catalogue::load(
            &mut store,
            Limits {
                max_bindings: 13,
                ..Default::default()
            }
        )
        .unwrap()
        .counts()
        .bindings,
        13
    );
}

#[test]
fn linked_reference_cycles_keep_master_and_self_identity_across_cache_and_order_changes() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            targets(),
            disk(
                b"ACHR",
                0x100,
                0,
                15,
                &[core(0x800, 0), link(b"XLKR", 0x101)].concat(),
            ),
            disk(
                b"ACHR",
                0x101,
                0,
                15,
                &[core(0x800, 0), link(b"XLKR", 0x100)].concat(),
            ),
        ]
        .concat(),
    )
    .unwrap();
    for name in ["A.esm", "B.esm"] {
        fs::write(
            directory.path().join(name),
            [
                header(&["FalloutNV.esm"]),
                disk(
                    b"ACHR",
                    0x0100_0100,
                    plugin::COMPRESSED,
                    15,
                    &[core(0x800, 0), link(b"XLKR", 0x0100_0101)].concat(),
                ),
                disk(
                    b"ACHR",
                    0x0100_0101,
                    0,
                    15,
                    &[
                        core(0x800, 0),
                        link(b"XLKR", 0x0100_0100),
                        link(b"XLKR", 0x100),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        )
        .unwrap();
    }
    let cache = tempfile::tempdir().unwrap();
    let mut observed = Vec::new();
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
        assert_eq!(catalogue.counts().records, 6);
        assert_eq!(catalogue.counts().bindings, 14);
        for (key, definition) in catalogue.iter() {
            let Value::LinkedReference {
                reference,
                schema_kind_allowed,
            } = &definition.fields[3].value
            else {
                panic!("link")
            };
            assert_eq!(*schema_kind_allowed, Some(true));
            assert_eq!(reference.status, Status::Defined);
            assert_eq!(
                reference.key.as_ref().unwrap().origin_plugin,
                key.origin_plugin
            );
            assert_eq!(
                reference
                    .target
                    .as_ref()
                    .unwrap()
                    .source_plugin
                    .to_lowercase(),
                key.origin_plugin
            );
            assert_eq!(
                reference.key.as_ref().unwrap().local_id,
                if key.local_id == 0x100 { 0x101 } else { 0x100 }
            );
            if key.origin_plugin != "falloutnv.esm" && key.local_id == 0x101 {
                let Value::LinkedReference { reference, .. } = &definition.fields[4].value else {
                    panic!("master link")
                };
                assert_eq!(
                    reference.key.as_ref().unwrap().origin_plugin,
                    "falloutnv.esm"
                );
                assert_eq!(
                    reference.target.as_ref().unwrap().source_plugin,
                    "FalloutNV.esm"
                );
                assert_eq!(
                    definition.findings[0].code,
                    "multiple_placed_linked_reference_fields"
                );
                assert_eq!(definition.findings.len(), 1);
            } else {
                assert!(definition.findings.is_empty());
            }
        }
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
}

#[test]
fn budgets_precede_candidate_clones_fields_core_maps_and_binding_allocations() {
    let directory = tempfile::tempdir().unwrap();
    let body = [core(0x800, 0), link(b"XEZN", 0x900)].concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            targets(),
            disk(b"ACHR", 0x100, plugin::COMPRESSED, 15, &body),
            disk(b"ACHR", 0x101, 0, 15, &body),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
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
            max_record_bytes: body.len() - 1,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: body.len() * 2 - 1,
            ..Default::default()
        },
        Limits {
            max_fields: 0,
            ..Default::default()
        },
        Limits {
            max_fields: 7,
            ..Default::default()
        },
        Limits {
            max_bindings: 0,
            ..Default::default()
        },
        Limits {
            max_bindings: 3,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut store, limits).is_err());
    }
    assert_eq!(
        Catalogue::load(
            &mut store,
            Limits {
                max_records: 2,
                ..Default::default()
            }
        )
        .unwrap()
        .counts()
        .records,
        2
    );
}

#[test]
fn winning_parent_move_tombstone_and_current_self_namespace_survive_cache_and_reordering() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            targets(),
            cell(0x2000, b"RoomA\0"),
            cell(0x2001, b"RoomB\0"),
            children(
                0x2000,
                &[
                    disk(b"ACHR", 0x100, 0, 15, &core(0x800, 0)),
                    disk(b"ACHR", 0x101, 0, 15, &core(0x800, 0)),
                ]
                .concat(),
            ),
        ]
        .concat(),
    )
    .unwrap();
    for name in ["A.esm", "B.esm"] {
        let mut rows = [
            header(&["FalloutNV.esm"]),
            disk(b"ECZN", 0x0100_0900, 0, 15, &[]),
            children(
                0x2000,
                &disk(
                    b"ACRE",
                    0x0100_0100,
                    plugin::COMPRESSED,
                    11,
                    &[core(0x801, 0), link(b"XEZN", 0x0100_0900)].concat(),
                ),
            ),
        ]
        .concat();
        if name == "A.esm" {
            rows.extend(children(
                0x2001,
                &disk(b"ACHR", 0x100, 0, 15, &core(0x800, 0x8000_0000)),
            ));
            rows.extend(children(
                0x2000,
                &disk(b"ACHR", 0x101, plugin::DELETED, 16, b"unread malformed"),
            ));
        }
        fs::write(directory.path().join(name), rows).unwrap();
    }
    let cache = tempfile::tempdir().unwrap();
    let mut observed = Vec::new();
    let mut digests = Vec::new();
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
        for (key, definition) in catalogue.iter() {
            if key.origin_plugin == "falloutnv.esm" && key.local_id == 0x100 {
                assert_eq!(definition.parent.cell, Some(0x2001));
                assert_eq!(definition.parent.child_group, Some(9));
                assert_eq!(definition.source.plugin, "A.esm");
            }
            if definition.deleted {
                assert!(
                    definition.core.is_none()
                        && definition.base.is_none()
                        && definition.record().is_none()
                        && definition.placement().is_none()
                        && definition.fields.is_empty()
                        && definition.findings.is_empty()
                );
                assert_eq!(definition.header.version, 16);
            }
            if key.origin_plugin != "falloutnv.esm" {
                let Value::EncounterZone { zone, .. } = &definition.fields[3].value else {
                    panic!("zone")
                };
                assert_eq!(zone.key.as_ref().unwrap().origin_plugin, key.origin_plugin);
                assert_eq!(
                    zone.target.as_ref().unwrap().source_plugin.to_lowercase(),
                    key.origin_plugin
                );
            }
        }
        digests.push(catalogue.winning_content_sha256().to_owned());
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
    assert_eq!(digests[0], digests[1]);
    assert_eq!(digests[0], digests[2]);
}
