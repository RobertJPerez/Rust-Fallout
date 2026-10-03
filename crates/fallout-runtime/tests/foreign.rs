mod common;
use common::{field, form, header, record, unit};
use fallout_data::{
    loaded_scripts::{Catalogue, Limits as CatalogueLimits},
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Error, Limits, World,
    events::Context,
    foreign::{Content, Failure, Request},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    state::InstanceHandle,
};
use std::{fs, num::NonZeroU64, path::Path};

fn fixture(path: &Path) {
    let source = unit(
        &[(42, 0), (90, 0)],
        &[
            (b"SCRO", 0x100),
            (b"SCRO", 0x101),
            (b"SCRO", 0x102),
            (b"SCRO", 0),
            (b"SCRO", 0x14),
            (b"SCRO", 0x103),
            (b"SCRO", 0x104),
            (b"SCRV", 90),
            (b"SCRV", 999),
        ],
    );
    let target = unit(&[(42, 0), (42, 1), (70, 0), (0, 0)], &[(b"SCRV", 70)]);
    let mut bytes = header(&[]);
    bytes.extend(record(b"SCPT", 0x300, 0, &source));
    bytes.extend(record(b"SCPT", 0x301, 0, &target));
    bytes.extend(record(b"SCPT", 0x302, 0, &unit(&[(42, 1)], &[])));
    bytes.extend(record(
        b"QUST",
        0x100,
        0,
        &field(b"SCRI", &0x301_u32.to_le_bytes()),
    ));
    bytes.extend(record(
        b"REFR",
        0x101,
        0,
        &field(b"NAME", &0x102_u32.to_le_bytes()),
    ));
    bytes.extend(record(
        b"ACTI",
        0x102,
        0,
        &field(b"SCRI", &0x301_u32.to_le_bytes()),
    ));
    bytes.extend(record(b"QUST", 0x103, plugin::DELETED, &[]));
    fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
}
fn load(path: &Path, maximum: usize) -> (Catalogue, Content) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, maximum).unwrap();
    (catalogue, content)
}
fn definition(catalogue: &Catalogue, id: u32) -> fallout_data::loaded_scripts::Handle {
    catalogue
        .record_scripts(&form(id))
        .next()
        .unwrap()
        .handle()
        .clone()
}
fn world(catalogue: &Catalogue) -> World<'_> {
    World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x30; 16]).unwrap(),
    )
    .unwrap()
}
fn source(world: &mut World<'_>, catalogue: &Catalogue) -> InstanceHandle {
    let handle = world
        .create_instance(
            &definition(catalogue, 0x300),
            Owner::Fragment {
                activation: NonZeroU64::new(1).unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 123 })])
        .unwrap();
    handle
}
fn request(source: InstanceHandle, context_reference: u16, local_index: u16) -> Request {
    Request {
        source,
        context_reference,
        local_index,
        player: None,
    }
}

#[test]
fn quest_uses_the_live_list_definition_and_never_the_caller_or_static_attachment() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let (catalogue, content) = load(directory.path(), 100);
    let mut world = world(&catalogue);
    let source = source(&mut world, &catalogue);
    assert!(matches!(
        world.read_foreign(&content, request(source, 1, 42)),
        Err(Failure::MissingEventList(_))
    ));
    // SCRI points to 301, but this explicitly installed live list uses 302.
    let target = world
        .create_instance(
            &definition(&catalogue, 0x302),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    assert!(matches!(
        world.read_foreign(&content, request(source, 1, 42)),
        Err(Failure::State(Error::UninitializedLocal(42)))
    ));
    world
        .assign(target, &[(42, Value::Number { bits: 456 })])
        .unwrap();
    let before = world.snapshot();
    let read = world
        .read_foreign(&content, request(source, 1, 42))
        .unwrap();
    assert_eq!(read.value, Value::Number { bits: 456 });
    assert_eq!(read.target.target_definition, definition(&catalogue, 0x302));
    assert_eq!(
        read.target.target_instance,
        world.instance(target).unwrap().id()
    );
    assert_eq!(world.snapshot(), before);
    assert!(matches!(
        world.foreign_target(&content, request(source, 1, 70)),
        Err(Failure::State(Error::MissingLocal(70)))
    ));
    world.remove_instance(target).unwrap();
    assert!(matches!(
        world.read_foreign(&content, request(source, 1, 42)),
        Err(Failure::MissingEventList(_))
    ));
    let replacement = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            replacement,
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_0123_4567_89ab,
                },
            )],
        )
        .unwrap();
    let read = world
        .read_foreign(&content, request(source, 1, 42))
        .unwrap();
    assert_eq!(
        read.value,
        Value::Number {
            bits: 0x7ff8_0123_4567_89ab
        }
    );
    assert_eq!(
        read.target.declaration.kind,
        fallout_runtime::schema::Kind::Float
    );
}

#[test]
fn placed_context_requires_a_registered_reference_and_its_live_list() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let (catalogue, content) = load(directory.path(), 100);
    let mut world = world(&catalogue);
    let source = source(&mut world, &catalogue);
    assert!(matches!(
        world.read_foreign(&content, request(source, 2, 42)),
        Err(Failure::ReferenceNotRegistered(_))
    ));
    let reference = world.register_reference(Some(form(0x101))).unwrap();
    assert!(matches!(
        world.read_foreign(&content, request(source, 2, 42)),
        Err(Failure::MissingEventList(_))
    ));
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Placed { reference },
            Context::default(),
        )
        .unwrap();
    world
        .assign(target, &[(42, Value::Number { bits: 789 })])
        .unwrap();
    let read = world
        .read_foreign(&content, request(source, 2, 42))
        .unwrap();
    assert_eq!(read.value, Value::Number { bits: 789 });
    assert_eq!(read.target.target_owner, Owner::Placed { reference });
    // ACTI's base script is available, but an ACTI is not a live event-list owner.
    assert!(matches!(
        world.read_foreign(&content, request(source, 3, 42)),
        Err(Failure::UnsupportedForm { .. })
    ));
}

#[test]
fn dynamic_context_and_player_binding_select_exact_typed_reference_values() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let (catalogue, content) = load(directory.path(), 100);
    let mut world = world(&catalogue);
    let source = source(&mut world, &catalogue);
    assert!(matches!(
        world.read_foreign(&content, request(source, 8, 42)),
        Err(Failure::State(Error::UninitializedLocal(90)))
    ));
    let player = world.register_reference(None).unwrap();
    let target = world
        .create_instance(
            &definition(&catalogue, 0x302),
            Owner::Placed { reference: player },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            target,
            &[(
                42,
                Value::Number {
                    bits: 0x8000_0000_0000_0000,
                },
            )],
        )
        .unwrap();
    for value in [
        ReferenceValue::Live { id: player },
        ReferenceValue::Content { key: form(0x14) },
    ] {
        world
            .assign(source, &[(90, Value::Reference { value })])
            .unwrap();
        let mut request = request(source, 8, 42);
        request.player = Some(player);
        assert_eq!(
            world.read_foreign(&content, request).unwrap().value,
            Value::Number {
                bits: 0x8000_0000_0000_0000
            }
        );
    }
    let mut explicit = request(source, 5, 42);
    explicit.player = Some(player);
    assert_eq!(
        world
            .read_foreign(&content, explicit)
            .unwrap()
            .target
            .target_instance,
        world.instance(target).unwrap().id()
    );
    assert!(matches!(
        world.read_foreign(&content, request(source, 5, 42)),
        Err(Failure::State(Error::UnresolvedDependency(_)))
    ));
    world
        .assign(
            source,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Null,
                },
            )],
        )
        .unwrap();
    assert!(matches!(
        world.read_foreign(&content, request(source, 8, 42)),
        Err(Failure::NullContext)
    ));
}

#[test]
fn invalid_contexts_never_alias_current_locals_or_invent_targets() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let (catalogue, content) = load(directory.path(), 100);
    let mut world = world(&catalogue);
    let source = source(&mut world, &catalogue);
    for (context, expected) in [
        (0, "unresolved_context_reference"),
        (4, "null_context"),
        (6, "unresolved_context_reference"),
        (7, "unresolved_context_reference"),
        (9, "unresolved_context_reference"),
        (u16::MAX, "unresolved_context_reference"),
    ] {
        let before = world.snapshot();
        assert_eq!(
            world
                .read_foreign(&content, request(source, context, 42))
                .unwrap_err()
                .code(),
            expected
        );
        assert_eq!(world.snapshot(), before);
    }
    for (key, expected) in [
        (form(0x103), "deleted_form"),
        (form(0x104), "missing_form"),
        (form(0x102), "unsupported_form_kind"),
    ] {
        world
            .assign(
                source,
                &[(
                    90,
                    Value::Reference {
                        value: ReferenceValue::Content { key },
                    },
                )],
            )
            .unwrap();
        assert_eq!(
            world
                .read_foreign(&content, request(source, 8, 42))
                .unwrap_err()
                .code(),
            expected
        );
    }
    let invalid_reference = world.register_reference(Some(form(0x100))).unwrap();
    world
        .assign(
            source,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live {
                        id: invalid_reference,
                    },
                },
            )],
        )
        .unwrap();
    assert_eq!(
        world
            .read_foreign(&content, request(source, 8, 42))
            .unwrap_err()
            .code(),
        "unsupported_form_kind"
    );
}

#[test]
fn foreign_writes_validate_the_target_schema_and_change_only_that_bank() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let (catalogue, content) = load(directory.path(), 100);
    let mut world = world(&catalogue);
    let source = source(&mut world, &catalogue);
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    assert!(
        world
            .assign_foreign(&content, request(source, 1, 70), Value::Number { bits: 2 })
            .is_err()
    );
    assert!(
        world
            .assign_foreign(&content, request(source, 1, 0), Value::Number { bits: 2 })
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    world
        .assign_foreign(
            &content,
            request(source, 1, 42),
            Value::Number { bits: u64::MAX },
        )
        .unwrap();
    assert_eq!(
        world.instance(target).unwrap().local(42).unwrap(),
        &Value::Number { bits: u64::MAX }
    );
    assert_eq!(
        world.instance(source).unwrap().local(42).unwrap(),
        &Value::Number { bits: 123 }
    );
    assert_eq!(world.revision(), before.state_revision + 1);
}

#[test]
fn restore_rebuilds_live_links_and_rejects_pre_restore_handles() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let (catalogue, content) = load(directory.path(), 100);
    let mut world = world(&catalogue);
    let source = source(&mut world, &catalogue);
    let target = world
        .create_instance(
            &definition(&catalogue, 0x302),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(target, &[(42, Value::Number { bits: 17 })])
        .unwrap();
    let source_id = world.instance(source).unwrap().id();
    let restored = World::restore(&catalogue, world.snapshot(), Limits::default()).unwrap();
    assert_eq!(
        restored
            .read_foreign(&content, request(source, 1, 42))
            .unwrap_err()
            .code(),
        "stale_handle"
    );
    assert_eq!(
        restored
            .read_foreign(
                &content,
                request(restored.handle(source_id).unwrap(), 1, 42)
            )
            .unwrap()
            .value,
        Value::Number { bits: 17 }
    );
}

#[test]
fn context_index_is_bounded_and_binds_all_sources_and_winners() {
    let first = tempfile::tempdir().unwrap();
    fixture(first.path());
    let (catalogue, content) = load(first.path(), 100);
    assert_eq!(content.report().counts.forms, 7);
    assert_eq!(content.report().counts.quests, 2);
    assert_eq!(content.report().counts.placed, 1);
    assert_eq!(content.report().counts.deleted, 1);
    let mut store = RecordStore::open_nv_headers(
        first.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    assert!(Content::load(&mut store, &catalogue, 6).is_err());
    drop(store);
    let second = tempfile::tempdir().unwrap();
    fixture(second.path());
    let mut bytes = fs::read(second.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"ACTI", 0x500, 0, &[]));
    fs::write(second.path().join("FalloutNV.esm"), bytes).unwrap();
    let (other_catalogue, other_content) = load(second.path(), 100);
    let mut mixed = RecordStore::open_nv_headers(
        second.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    assert!(Content::load(&mut mixed, &catalogue, 100).is_err());
    let mut world = world(&other_catalogue);
    let source = source(&mut world, &other_catalogue);
    assert_eq!(
        world
            .read_foreign(&content, request(source, 1, 42))
            .unwrap_err()
            .code(),
        "content_changed"
    );
    assert!(
        world
            .read_foreign(&other_content, request(source, 1, 42))
            .is_err()
    );
}

#[test]
fn master_namespaces_and_winning_target_schemas_remain_distinct() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut addon = header(&["FalloutNV.esm"]);
    addon.extend(record(
        b"SCPT",
        0x0100_0303,
        0,
        &unit(
            &[(42, 1)],
            &[(b"SCRO", 0x0100_0100), (b"SCRO", 0x0000_0100)],
        ),
    ));
    addon.extend(record(
        b"QUST",
        0x0100_0100,
        0,
        &field(b"SCRI", &0x0100_0303_u32.to_le_bytes()),
    ));
    // Override the base script's numeric declaration with a typed reference.
    addon.extend(record(
        b"SCPT",
        0x0000_0302,
        0,
        &unit(&[(42, 0)], &[(b"SCRV", 42)]),
    ));
    fs::write(directory.path().join("Addon.esm"), addon).unwrap();
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into(), "Addon.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let catalogue = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    let addon_form = |local_id| fallout_data::identity::FormKey {
        origin_plugin: "addon.esm".into(),
        local_id,
        ..form(local_id)
    };
    let addon_definition = catalogue
        .record_scripts(&addon_form(0x303))
        .next()
        .unwrap()
        .handle()
        .clone();
    let mut world = world(&catalogue);
    let source = world
        .create_instance(
            &addon_definition,
            Owner::Fragment {
                activation: NonZeroU64::new(1).unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    let addon_target = world
        .create_instance(
            &addon_definition,
            Owner::Quest {
                key: addon_form(0x100),
            },
            Context::default(),
        )
        .unwrap();
    let base_target = world
        .create_instance(
            &definition(&catalogue, 0x302),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(addon_target, &[(42, Value::Number { bits: 1234 })])
        .unwrap();
    world
        .assign(
            base_target,
            &[(
                42,
                Value::Reference {
                    value: ReferenceValue::Null,
                },
            )],
        )
        .unwrap();
    assert_eq!(
        world
            .read_foreign(&content, request(source, 1, 42))
            .unwrap()
            .value,
        Value::Number { bits: 1234 }
    );
    assert_eq!(
        world
            .read_foreign(&content, request(source, 2, 42))
            .unwrap()
            .value,
        Value::Reference {
            value: ReferenceValue::Null
        }
    );
    assert_eq!(
        world
            .foreign_target(&content, request(source, 2, 42))
            .unwrap()
            .target_definition,
        definition(&catalogue, 0x302)
    );
}

#[test]
fn equal_winning_headers_cannot_hide_changed_source_bodies() {
    let first = tempfile::tempdir().unwrap();
    fixture(first.path());
    let (catalogue, content) = load(first.path(), 100);
    let second = tempfile::tempdir().unwrap();
    fixture(second.path());
    let path = second.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&path).unwrap();
    // ACTI's final SCRI value changes without changing any header or extent.
    let end = bytes.len() - record(b"QUST", 0x103, plugin::DELETED, &[]).len();
    bytes[end - 4..end].copy_from_slice(&0x302_u32.to_le_bytes());
    fs::write(path, bytes).unwrap();
    let mut changed = RecordStore::open_nv_headers(
        second.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    assert_eq!(
        fallout_data::record_metadata::inspect(&changed)
            .unwrap()
            .winning_definitions_sha256,
        content.report().winning_headers_sha256
    );
    assert!(Content::load(&mut changed, &catalogue, 100).is_err());
}
