use fallout_data::{
    actors::{
        self,
        script_attachment::{self, Limits, Status},
    },
    identity::{FormKey, ProfileId},
    inventory, loaded_scripts, plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

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
    [
        kind.as_slice(),
        &(raw.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        raw,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut raw = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        raw.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        raw.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, &raw)
}
fn configuration(template: u16) -> Vec<u8> {
    let mut raw = [0; 24];
    raw[22..24].copy_from_slice(&template.to_le_bytes());
    field(b"ACBS", &raw)
}
fn actor(creature: bool, template: u16, extra: Vec<u8>) -> Vec<u8> {
    [
        configuration(template),
        field(b"DATA", &vec![0; if creature { 17 } else { 11 }]),
        extra,
    ]
    .concat()
}
fn link(id: u32) -> Vec<u8> {
    field(b"SCRI", &id.to_le_bytes())
}
fn script(compiled: bool, variant: u8, text: bool) -> Vec<u8> {
    let mut hdr = [0; 20];
    hdr[4..8].copy_from_slice(&5u32.to_le_bytes());
    hdr[8..12].copy_from_slice(&if compiled { 4u32 } else { 0 }.to_le_bytes());
    hdr[12..16].copy_from_slice(&2u32.to_le_bytes());
    hdr[18..20].copy_from_slice(&0x8001u16.to_le_bytes());
    let mut raw = field(b"SCHR", &hdr);
    if compiled {
        raw.extend(field(b"SCDA", &[0x11 + variant, 0, 0, 0]));
    }
    if text {
        raw.extend(field(b"SCTX", b"arbitrary uncompiled author text\0"));
    }
    for (name, ty) in [(b"first_\xe9\0".as_slice(), 0), (b"second\0".as_slice(), 1)] {
        let mut decl = [0xa5; 24];
        decl[..4].copy_from_slice(&77u32.to_le_bytes());
        decl[16] = ty;
        raw.extend(field(b"SLSD", &decl));
        raw.extend(field(b"SCVR", name));
    }
    for (kind, id) in [
        (b"SCRO", 0x200u32),
        (b"SCRO", 0x201),
        (b"SCRO", 0x999),
        (b"SCRO", 0),
        (b"SCRV", 77),
    ] {
        raw.extend(field(kind, &id.to_le_bytes()));
    }
    raw
}
fn fixture(path: &Path, variant: u8) {
    fs::create_dir_all(path).unwrap();
    let mut bytes = header(&[]);
    for (id, creature, flags, raw) in [
        (0x100, false, 0, actor(false, 0, link(0x300))),
        (0x101, true, 0, actor(true, 0, link(0x300))),
        (0x102, false, 0, actor(false, 0, Vec::new())),
        (
            0x103,
            false,
            0,
            actor(false, 0, [link(0x300), link(0x301)].concat()),
        ),
        (0x104, false, 0, actor(false, 0, link(0))),
        (0x105, false, 0, actor(false, 0, link(0x999))),
        (0x106, false, 0, actor(false, 0, link(0x301))),
        (0x107, false, 0, actor(false, 0, link(0x200))),
        (0x108, false, 0, actor(false, 0, link(0x302))),
        (0x109, false, 0, actor(false, 0, link(0x303))),
        (0x10a, false, 0, actor(false, 0, link(0x304))),
        (0x10b, false, 0, actor(false, 512, link(0x300))),
        (
            0x10c,
            false,
            0,
            [field(b"DATA", &[0; 11]), link(0x300)].concat(),
        ),
        (
            0x10d,
            false,
            0,
            [
                configuration(0),
                configuration(512),
                field(b"DATA", &[0; 11]),
                link(0x300),
            ]
            .concat(),
        ),
        (0x10e, false, 0, actor(false, 0, field(b"SCRI", &[0, 3, 0]))),
        (0x10f, false, plugin::DELETED, Vec::new()),
    ] {
        bytes.extend(disk(
            if creature { b"CREA" } else { b"NPC_" },
            id,
            flags,
            &raw,
        ));
    }
    bytes.extend(disk(b"ARMO", 0x200, 0, &[]));
    bytes.extend(disk(b"FACT", 0x201, plugin::DELETED, &[]));
    bytes.extend(disk(b"SCPT", 0x300, 0, &script(true, variant, false)));
    bytes.extend(disk(b"SCPT", 0x301, plugin::DELETED, &[]));
    bytes.extend(disk(b"SCPT", 0x302, 0, &[]));
    bytes.extend(disk(
        b"SCPT",
        0x303,
        0,
        &[script(true, 0, false), script(true, 1, false)].concat(),
    ));
    bytes.extend(disk(b"SCPT", 0x304, 0, &script(false, 0, true)));
    fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
    fs::write(
        path.join("override.esp"),
        [
            header(&["FalloutNV.esm"]),
            disk(b"SCPT", 0x300, 0, &script(true, 2, true)),
            disk(b"NPC_", 0x100, 0, &actor(false, 0, link(0x304))),
        ]
        .concat(),
    )
    .unwrap();
}
fn order(overrides: bool) -> Vec<String> {
    if overrides {
        vec!["FalloutNV.esm".into(), "override.esp".into()]
    } else {
        vec!["FalloutNV.esm".into()]
    }
}
fn source(path: &Path, overrides: bool) -> RecordStore {
    RecordStore::open_nv_headers(path, &order(overrides), Default::default()).unwrap()
}
fn with_inputs<T>(
    path: &Path,
    overrides: bool,
    action: impl FnOnce(&mut RecordStore, &actors::Catalogue<'_>, &loaded_scripts::Catalogue) -> T,
) -> T {
    let mut store = source(path, overrides);
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    action(&mut store, &actors, &scripts)
}

#[test]
fn exact_physical_link_borrows_existing_definition_and_preserves_all_declarations() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors, scripts| {
        for id in [0x100, 0x101] {
            let request =
                script_attachment::request(store, actors, scripts, &key(id), Default::default())
                    .unwrap();
            assert_eq!(request.status, Status::LoadedDefinition);
            assert_eq!(request.selected_definition, Some(0));
            assert_eq!(request.template_script_flag, Some(false));
            assert!(!request.execution_supported);
            assert_eq!(request.attachments.len(), 1);
            let link = &request.attachments[0];
            assert_eq!(link.field_index, 2);
            assert_eq!(link.field_decoded_offset, if id == 0x100 { 47 } else { 53 });
            assert_eq!(link.raw_bytes, 0x300u32.to_le_bytes());
            assert!(!link.repeated);
            assert_eq!(link.binding.as_ref().unwrap().key, Some(key(0x300)));
            let compiled = &request.compiled_definitions[0];
            let loaded = scripts.record_scripts(&key(0x300)).next().unwrap();
            assert!(std::ptr::eq(compiled.handle, loaded.handle()));
            assert!(std::ptr::eq(compiled.version, loaded.version()));
            assert!(std::ptr::eq(compiled.declarations, loaded.declarations()));
            assert_eq!(compiled.owner.kind, loaded_scripts::OwnerKind::Standalone);
            assert!(compiled.owner.schema_ownership_verified);
            assert_eq!(compiled.flags, 0x8001);
            assert_eq!(
                compiled
                    .declarations
                    .iter()
                    .map(|d| d.index)
                    .collect::<Vec<_>>(),
                vec![77, 77]
            );
            assert_eq!(compiled.references.len(), 5);
            assert_eq!(request.field_visits, 9);
            assert_eq!(request.attachment_bytes, 4);
            assert_eq!(request.declarations, 2);
            assert_eq!(request.references, 5);
        }
    });
}
#[test]
fn unavailable_links_units_and_template_declarations_never_select_a_first_winner() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors, scripts| {
        for (id, status) in [
            (0x102, Status::NoScriptField),
            (0x103, Status::MultipleScriptFields),
            (0x104, Status::NullScript),
            (0x105, Status::MissingScript),
            (0x106, Status::DeletedScript),
            (0x107, Status::WrongScriptKind),
            (0x108, Status::MissingLoadedDefinition),
            (0x109, Status::MultipleStandaloneUnits),
            (0x10a, Status::MissingCompiledBody),
            (0x10b, Status::TemplateInheritanceUnsupported),
            (0x10c, Status::MissingConfiguration),
            (0x10d, Status::MultipleConfigurations),
            (0x10e, Status::UnsupportedScriptLayout),
        ] {
            let request =
                script_attachment::request(store, actors, scripts, &key(id), Default::default())
                    .unwrap();
            assert_eq!(request.status, status, "{id:x}");
            assert_eq!(request.selected_definition, None);
            assert!(!request.execution_supported);
            if id == 0x103 {
                assert!(request.attachments.iter().all(|f| f.repeated));
                assert!(request.compiled_definitions.is_empty());
            }
            if id == 0x109 {
                assert_eq!(request.compiled_definitions.len(), 2);
            }
            if id == 0x10b {
                assert_eq!(request.template_script_flag, Some(true));
                assert_eq!(request.compiled_definitions.len(), 1);
            }
        }
        for id in [0x10f, 0x200, 0xffff] {
            assert!(
                script_attachment::request(store, actors, scripts, &key(id), Default::default())
                    .is_err()
            );
        }
    });
}
#[test]
fn actor_and_script_winning_overrides_change_exact_source_and_selection() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let base = with_inputs(temp.path(), false, |store, actors, scripts| {
        serde_json::to_value(
            script_attachment::request(store, actors, scripts, &key(0x101), Default::default())
                .unwrap(),
        )
        .unwrap()
    });
    with_inputs(temp.path(), true, |store, actors, scripts| {
        let creature =
            script_attachment::request(store, actors, scripts, &key(0x101), Default::default())
                .unwrap();
        assert_eq!(creature.status, Status::LoadedDefinition);
        assert_eq!(
            creature.compiled_definitions[0].version.source_plugin,
            "override.esp"
        );
        assert_ne!(
            creature.compiled_definitions[0].handle.version_sha256,
            base["compiled_definitions"][0]["handle"]["version_sha256"]
        );
        let actor =
            script_attachment::request(store, actors, scripts, &key(0x100), Default::default())
                .unwrap();
        assert_eq!(actor.actor.source.plugin, "override.esp");
        assert_eq!(actor.status, Status::MissingCompiledBody);
        assert_eq!(
            actor.attachments[0].binding.as_ref().unwrap().key,
            Some(key(0x304))
        );
    });
}
#[test]
fn identical_headers_with_changed_script_body_cannot_supply_the_store_cohort() {
    let original = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    fixture(original.path(), 0);
    fixture(changed.path(), 1);
    with_inputs(original.path(), false, |_, actors, scripts| {
        let mut other = source(changed.path(), false);
        assert!(
            script_attachment::request(
                &mut other,
                actors,
                scripts,
                &key(0x100),
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("cohort")
        );
    });
    with_inputs(original.path(), false, |store, actors, _| {
        let mut other = source(changed.path(), false);
        let other_scripts =
            loaded_scripts::Catalogue::load(&mut other, Default::default(), |_, _| Ok(())).unwrap();
        assert!(
            script_attachment::request(
                store,
                actors,
                &other_scripts,
                &key(0x100),
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("cohort")
        );
    });
}
#[test]
fn caller_mutated_receipt_rows_cannot_relabel_retained_actor_bytes() {
    let original = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    fixture(original.path(), 0);
    fixture(changed.path(), 1);
    let mut old_store = source(original.path(), false);
    let mut inventory = inventory::Catalogue::load(&mut old_store, Default::default()).unwrap();
    let mut current_store = source(changed.path(), false);
    inventory.sources = current_store.source_receipts().unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut current_store, Default::default(), |_, _| Ok(()))
            .unwrap();
    let error = script_attachment::request(
        &mut current_store,
        &actors,
        &scripts,
        &key(0x100),
        Default::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("winning actor"));
}

#[test]
fn every_selected_work_and_projection_limit_admits_exact_and_refuses_one_under() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors, scripts| {
        let request =
            script_attachment::request(store, actors, scripts, &key(0x100), Default::default())
                .unwrap();
        let bytes = serde_json::to_vec(&request).unwrap().len();
        let exact = Limits {
            max_sources: 1,
            max_field_visits: 9,
            max_attachments: 1,
            max_attachment_bytes: 4,
            max_matching_units: 1,
            max_declarations: 2,
            max_references: 5,
            max_projection_bytes: bytes,
        };
        assert!(script_attachment::request(store, actors, scripts, &key(0x100), exact).is_ok());
        for smaller in [
            Limits {
                max_sources: 0,
                ..exact
            },
            Limits {
                max_field_visits: 8,
                ..exact
            },
            Limits {
                max_attachments: 0,
                ..exact
            },
            Limits {
                max_attachment_bytes: 3,
                ..exact
            },
            Limits {
                max_matching_units: 0,
                ..exact
            },
            Limits {
                max_declarations: 1,
                ..exact
            },
            Limits {
                max_references: 4,
                ..exact
            },
            Limits {
                max_projection_bytes: bytes - 1,
                ..exact
            },
        ] {
            assert!(
                script_attachment::request(store, actors, scripts, &key(0x100), smaller).is_err()
            );
        }
    });
}
#[test]
fn export_optional_source_fixture_and_existing_host_requests() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_SCRIPT_EVIDENCE_DIR") else {
        return;
    };
    let destination = Path::new(&destination);
    let install = destination.join("fixture");
    let data = install.join("Data");
    fixture(&data, 0);
    for overrides in [false, true] {
        let name = if overrides { "override" } else { "base" };
        fs::write(
            install.join(format!("{name}-order.json")),
            serde_json::to_vec(&order(overrides)).unwrap(),
        )
        .unwrap();
        with_inputs(&data, overrides, |store, actors, scripts| {
            for id in if overrides {
                vec![0x100, 0x101]
            } else {
                (0x100..=0x10e).collect()
            } {
                let request = script_attachment::request(
                    store,
                    actors,
                    scripts,
                    &key(id),
                    Default::default(),
                )
                .unwrap();
                fs::write(
                    install.join(format!("{name}-{id:x}-host.json")),
                    serde_json::to_vec(&request).unwrap(),
                )
                .unwrap();
            }
        });
    }
}
