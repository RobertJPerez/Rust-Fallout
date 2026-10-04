mod common;
use common::*;
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::equipment::{self, Error, Limits},
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    inventory::{Condition, Facts, ItemId, OpaqueExtra, Ownership},
    snapshot::Snapshot,
};
use std::{
    fs,
    num::{NonZeroU32, NonZeroU64},
    path::Path,
};

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut raw = record(kind, id, flags, body);
    raw[20..22].copy_from_slice(&15_u16.to_le_bytes());
    raw
}
fn fixture(path: &Path, marker: u8) {
    fs::create_dir_all(path.join("Data")).unwrap();
    fs::write(
        path.join("Data/FalloutNV.esm"),
        [
            header(&[]),
            disk(
                b"NPC_",
                0x100,
                0,
                &[field(b"ACBS", &[0; 24]), field(b"DATA", &[marker; 11])].concat(),
            ),
            disk(b"ARMO", 0x200, 0, &field(b"MODL", b"caller-chosen.nif\0")),
            disk(b"ARMO", 0x201, plugin::DELETED, b"unread tombstone"),
            disk(b"IMOD", 0x300, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(path.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(path.join("order.txt"), b"FalloutNV.esm").unwrap();
}
fn with_source(path: &Path, callback: impl FnOnce(&Catalogue, &Content)) {
    let mut store = RecordStore::open_nv_headers(
        &path.join("Data"),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 100).unwrap();
    callback(&scripts, &content);
}
fn world(scripts: &Catalogue) -> World<'_> {
    World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x16; 16]).unwrap(),
    )
    .unwrap()
}
fn facts(which: u8) -> Facts {
    let mut value = Facts::unknown(form(0x200));
    match which {
        0 => {
            value.condition = Some(Condition::Float32 { bits: 0x80000000 });
        }
        1 => {
            value.condition = Some(Condition::Float64 {
                bits: 0x7ff8000000000031,
            });
            value.equipped_slots = Some(vec![]);
            value.modifications = Some(vec![]);
            value.ownership = Some(Ownership::Unowned);
        }
        _ => {
            value.equipped_slots = Some(vec![7, 0, u16::MAX]);
            value.modifications = Some(vec![form(0x300), form(0x300)]);
            value.quest_item = Some(false);
            value.extra_fields = vec![OpaqueExtra {
                tag: *b"ZZZZ",
                bytes: vec![0, 255, 1],
            }];
        }
    }
    value
}
fn lots(world: &mut World<'_>) -> (ReferenceId, Vec<ItemId>) {
    let owner = world.register_reference(None).unwrap();
    world.initialize_inventory(owner).unwrap();
    let items = (0..3)
        .map(|index| {
            world
                .add_item(
                    owner,
                    facts(index),
                    NonZeroU32::new(u32::from(index) + 2).unwrap(),
                )
                .unwrap()
        })
        .collect();
    (owner, items)
}

#[test]
fn exact_same_base_lots_keep_condition_width_optional_slots_and_modification_keys() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_source(temp.path(), |scripts, content| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let before = world.snapshot();
        for (index, item) in items.into_iter().enumerate() {
            let selected =
                equipment::observe(&world, content, owner, item, Default::default()).unwrap();
            assert_eq!(selected.selected_lot().id(), item);
            assert_eq!(selected.selected_lot().owner(), owner);
            assert_eq!(selected.selected_lot().facts(), &facts(index as u8));
            assert_eq!(selected.selected_lot().count(), index as u32 + 2);
            assert_eq!(selected.base(), &form(0x200));
            assert_eq!(selected.source_form().kind, *b"ARMO");
            assert_eq!(selected.inventory().campaign(), world.campaign());
            assert_eq!(selected.inventory().revision(), world.revision());
            let json = serde_json::to_value(&selected).unwrap();
            assert_eq!(json["selected_item_index"], index);
            assert_eq!(json["active_mod_mask"], serde_json::Value::Null);
            assert_eq!(json["equip_rules_supported"], false);
        }
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn uninitialized_empty_wrong_owner_removed_and_missing_item_never_select_another_lot() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_source(temp.path(), |scripts, content| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let other = world.register_reference(None).unwrap();
        let before = world.snapshot();
        assert!(matches!(
            equipment::observe(&world, content, other, items[0], Default::default()),
            Err(Error::InventoryUninitialized)
        ));
        assert_eq!(world.snapshot(), before);
        world.initialize_inventory(other).unwrap();
        assert!(matches!(
            equipment::observe(&world, content, other, items[0], Default::default()),
            Err(Error::InventoryEmpty)
        ));
        let other_item = world
            .add_item(
                other,
                Facts::unknown(form(0x200)),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            equipment::observe(&world, content, other, items[0], Default::default()),
            Err(Error::WrongOwner)
        ));
        assert!(matches!(
            equipment::observe(&world, content, owner, other_item, Default::default()),
            Err(Error::WrongOwner)
        ));
        world
            .remove_item_quantity(items[0], NonZeroU32::new(2).unwrap())
            .unwrap();
        let before = world.snapshot();
        for item in [items[0], ItemId(NonZeroU64::new(999).unwrap())] {
            assert!(matches!(
                equipment::observe(&world, content, owner, item, Default::default()),
                Err(Error::ItemUnavailable)
            ));
        }
        assert!(matches!(
            equipment::observe(
                &world,
                content,
                ReferenceId(NonZeroU64::new(999).unwrap()),
                items[1],
                Default::default()
            ),
            Err(Error::State(fallout_runtime::Error::MissingReference))
        ));
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn cold_persistent_lot_is_exact_while_old_and_foreign_campaign_handles_refuse() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_source(temp.path(), |scripts, content| {
        let mut live = world(scripts);
        let (owner, items) = lots(&mut live);
        let handle = live.item_handle(items[2]).unwrap();
        let expected =
            equipment::observe_handle(&live, content, owner, handle, Default::default()).unwrap();
        let snapshot = live.snapshot();
        let bytes = snapshot
            .encode(WorldLimits::default().max_snapshot_bytes)
            .unwrap();
        let cold = World::restore(
            scripts,
            Snapshot::decode(&bytes, WorldLimits::default()).unwrap(),
            WorldLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            equipment::observe_handle(&cold, content, owner, handle, Default::default()),
            Err(Error::State(fallout_runtime::Error::StaleHandle))
        ));
        let actual =
            equipment::observe(&cold, content, owner, items[2], Default::default()).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(cold.snapshot(), snapshot);
        let mut foreign = World::with_campaign(
            scripts,
            WorldLimits::default(),
            CampaignId::from_bytes([0x17; 16]).unwrap(),
        )
        .unwrap();
        let (other_owner, other_items) = lots(&mut foreign);
        assert_eq!(other_items, items);
        assert!(matches!(
            equipment::observe_handle(&foreign, content, other_owner, handle, Default::default()),
            Err(Error::State(fallout_runtime::Error::StaleHandle))
        ));
    });
}

#[test]
fn deleted_missing_source_base_and_changed_content_cohort_refuse() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let changed = tempfile::tempdir().unwrap();
    fixture(changed.path(), 1);
    with_source(temp.path(), |scripts, content| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        for base in [0x201, 0x999] {
            let id = world
                .add_item(
                    owner,
                    Facts::unknown(form(base)),
                    NonZeroU32::new(1).unwrap(),
                )
                .unwrap();
            let before = world.snapshot();
            assert!(matches!(
                equipment::observe(&world, content, owner, id, Default::default()),
                Err(Error::Content(_))
            ));
            assert_eq!(world.snapshot(), before);
        }
        with_source(changed.path(), |_, other_content| {
            let before = world.snapshot();
            assert!(matches!(
                equipment::observe(&world, other_content, owner, items[0], Default::default()),
                Err(Error::Content(_))
            ));
            assert_eq!(world.snapshot(), before);
        });
    });
}

#[test]
fn aggregate_view_link_payload_item_and_selected_projection_visit_bounds_are_exact() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_source(temp.path(), |scripts, content| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let before = world.snapshot();
        let observed =
            equipment::observe(&world, content, owner, items[2], Default::default()).unwrap();
        let usage = observed.inventory().usage();
        let exact = Limits {
            inventory: fallout_runtime::inventory::ViewLimits {
                max_items: usage.items,
                max_links: usage.links,
                max_extra_bytes: usage.extra_bytes,
            },
            max_visits: observed.visits(),
            max_projection_bytes: serde_json::to_vec(&observed).unwrap().len(),
        };
        assert!(equipment::observe(&world, content, owner, items[2], exact).is_ok());
        for name in ["items", "links", "extra bytes", "visit", "projection byte"] {
            let mut under = exact;
            match name {
                "items" => under.inventory.max_items -= 1,
                "links" => under.inventory.max_links -= 1,
                "extra bytes" => under.inventory.max_extra_bytes -= 1,
                "visit" => under.max_visits -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                _ => unreachable!(),
            }
            let error = equipment::observe(&world, content, owner, items[2], under).unwrap_err();
            assert!(error.to_string().contains(name), "{error}");
        }
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn export_authored_cold_lot_sources_for_independent_actual_cli_when_requested() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_ITEM_EVIDENCE_DIR") else {
        return;
    };
    let path = std::path::PathBuf::from(destination);
    assert!(!path.join("fixture").exists());
    let path = path.join("fixture");
    fixture(&path, 0);
    with_source(&path, |scripts, content| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let unknown = world.register_reference(None).unwrap();
        let empty = world.register_reference(None).unwrap();
        world.initialize_inventory(empty).unwrap();
        let other = world.register_reference(None).unwrap();
        world.initialize_inventory(other).unwrap();
        let other_item = world
            .add_item(
                other,
                Facts::unknown(form(0x200)),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap();
        let snapshot = world.snapshot();
        fs::write(
            path.join("snapshot.json"),
            snapshot
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap(),
        )
        .unwrap();
        let cold = World::restore(scripts, snapshot.clone(), WorldLimits::default()).unwrap();
        for item in &items {
            let selected =
                equipment::observe(&cold, content, owner, *item, Default::default()).unwrap();
            fs::write(
                path.join(format!("host-item-{}.json", item.0)),
                serde_json::to_vec(&selected).unwrap(),
            )
            .unwrap();
        }
        fs::write(path.join("requests.json"),serde_json::to_vec(&serde_json::json!({"owner":owner,"items":items,"unknown":unknown,"empty":empty,"other":other,"other_item":other_item})).unwrap()).unwrap();
        assert_eq!(cold.snapshot(), snapshot);
    });
}

#[test]
#[ignore = "explicit private installed source request; heavy slot only"]
fn export_selected_installed_lots_through_canonical_apis() {
    let request_path = std::env::var_os("FALLOUT_ACTOR_ITEM_INSTALLED_REQUEST")
        .expect("explicit request required");
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(request_path).unwrap()).unwrap();
    let data = std::path::PathBuf::from(request["data"].as_str().unwrap());
    let names: Vec<String> = serde_json::from_value(request["load_order"].clone()).unwrap();
    let base: fallout_data::identity::FormKey =
        serde_json::from_value(request["base"].clone()).unwrap();
    let owner: ReferenceId = serde_json::from_value(request["owner"].clone()).unwrap();
    let destination = std::path::PathBuf::from(request["destination"].as_str().unwrap());
    assert!(!destination.exists());
    fs::create_dir_all(&destination).unwrap();
    let mut store = RecordStore::open_nv_headers(&data, &names, Default::default()).unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 2_000_000).unwrap();
    let snapshot = Snapshot::decode(
        &fs::read(request["snapshot"].as_str().unwrap()).unwrap(),
        WorldLimits::default(),
    )
    .unwrap();
    let mut live = World::restore(&scripts, snapshot, WorldLimits::default()).unwrap();
    assert_eq!(content.source_form(&live, &base).unwrap().kind, *b"ARMO");
    live.initialize_inventory(owner).unwrap();
    let mut items = Vec::new();
    for index in 0..3 {
        let mut supplied = facts(index);
        supplied.base = base.clone();
        supplied.modifications = None;
        items.push(
            live.add_item(
                owner,
                supplied,
                NonZeroU32::new(u32::from(index) + 2).unwrap(),
            )
            .unwrap(),
        );
    }
    let snapshot = live.snapshot();
    fs::write(
        destination.join("snapshot.json"),
        snapshot
            .encode(WorldLimits::default().max_snapshot_bytes)
            .unwrap(),
    )
    .unwrap();
    let cold = World::restore(&scripts, snapshot.clone(), WorldLimits::default()).unwrap();
    for item in &items {
        let selected =
            equipment::observe(&cold, &content, owner, *item, Default::default()).unwrap();
        fs::write(
            destination.join(format!("host-item-{}.json", item.0)),
            serde_json::to_vec(&selected).unwrap(),
        )
        .unwrap();
    }
    fs::write(
        destination.join("requests.json"),
        serde_json::to_vec(&serde_json::json!({"owner":owner,"items":items})).unwrap(),
    )
    .unwrap();
    assert_eq!(cold.snapshot(), snapshot);
}
