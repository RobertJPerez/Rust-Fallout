mod common;
use common::*;
use fallout_data::{
    actors::{
        self,
        dependencies::{Sex, equipment::Role},
    },
    assets::ArchiveAssets,
    inventory,
    loaded_scripts::Catalogue,
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::equipment_render::{Choice, Error, Limits, Requests},
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    inventory::{Condition, Facts, ItemId, OpaqueExtra},
    snapshot::Snapshot,
};
use std::{
    fs,
    num::{NonZeroU32, NonZeroU64},
    path::Path,
};

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = record(kind, id, flags, body);
    bytes[20..22].copy_from_slice(&15_u16.to_le_bytes());
    bytes
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
            disk(b"NPC_", 0x101, plugin::DELETED, b"unread tombstone"),
            disk(
                b"ARMO",
                0x200,
                0,
                &[
                    field(
                        b"BMDT",
                        &[u32::MAX.to_le_bytes().as_slice(), &[255, 1, 2, 3]].concat(),
                    ),
                    field(b"ETYP", &i32::MIN.to_le_bytes()),
                    field(b"MODL", b"male_biped.nif\0"),
                    field(b"MOD3", b"female_biped.nif\0"),
                    field(b"MOD2", b"male_world.nif\0"),
                    field(b"MOD4", b"female_world.nif\0"),
                ]
                .concat(),
            ),
            disk(
                b"WEAP",
                0x210,
                0,
                &[
                    field(b"ETYP", &(-1_i32).to_le_bytes()),
                    field(b"MODL", b"weapon_0.nif\0"),
                    field(b"MWD1", b"weapon_1.nif\0"),
                    field(b"WNAM", &0x300_u32.to_le_bytes()),
                    field(b"WNM1", &0x301_u32.to_le_bytes()),
                    field(b"MOD2", b"shell.nif\0"),
                    field(b"MOD3", b"scope.nif\0"),
                    field(b"MOD4", b"world.nif\0"),
                ]
                .concat(),
            ),
            disk(b"STAT", 0x300, 0, &field(b"MODL", b"first_0.nif\0")),
            disk(b"STAT", 0x301, 0, &field(b"MODL", b"first_1.nif\0")),
            disk(b"IMOD", 0x400, 0, &[]),
            disk(b"MISC", 0x500, 0, &[]),
            disk(b"ARMO", 0x201, plugin::DELETED, b"unread tombstone"),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(path.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
}
fn with_sources(
    path: &Path,
    callback: impl FnOnce(
        &mut RecordStore,
        &Catalogue,
        &Content,
        &actors::Catalogue<'_>,
        &ArchiveAssets,
    ),
) {
    let mut store = RecordStore::open_nv_headers(
        &path.join("Data"),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 100).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let assets = ArchiveAssets::open_nv(path).unwrap();
    callback(&mut store, &scripts, &content, &actors, &assets);
}
fn world(scripts: &Catalogue) -> World<'_> {
    World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x27; 16]).unwrap(),
    )
    .unwrap()
}
fn facts(index: usize, base: u32) -> Facts {
    let mut f = Facts::unknown(form(base));
    match index {
        0 => f.condition = Some(Condition::Float32 { bits: 0x8000_0000 }),
        1 => {
            f.condition = Some(Condition::Float64 {
                bits: 0x7ff8_0000_0000_0027,
            });
            f.equipped_slots = Some(vec![]);
            f.modifications = Some(vec![]);
        }
        _ => {
            f.equipped_slots = Some(vec![65535, 0, 7]);
            f.modifications = Some(vec![form(0x400), form(0x400)]);
            f.extra_fields = vec![OpaqueExtra {
                tag: *b"ZZZZ",
                bytes: vec![0, 255, 39],
            }];
        }
    }
    f
}
fn lots(world: &mut World<'_>) -> (ReferenceId, Vec<ItemId>) {
    let owner = world.register_reference(None).unwrap();
    world.initialize_inventory(owner).unwrap();
    let items = [0x200, 0x200, 0x200, 0x210, 0x210, 0x500]
        .into_iter()
        .enumerate()
        .map(|(index, base)| {
            world
                .add_item(
                    owner,
                    facts(index, base),
                    NonZeroU32::new(index as u32 + 2).unwrap(),
                )
                .unwrap()
        })
        .collect();
    (owner, items)
}
fn choice(world: &World<'_>, owner: ReferenceId, item: ItemId, role: Role) -> Choice {
    Choice {
        owner,
        item: world.item_handle(item).unwrap(),
        actor: form(0x100),
        role,
    }
}

#[test]
fn exact_lots_and_explicit_roles_join_without_deriving_equip_or_mod_state() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |store, scripts, content, actors, assets| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let before = world.snapshot();
        for (item, role, expected_path) in [
            (
                items[0],
                Role::ArmorBiped { sex: Sex::Female },
                b"female_biped.nif".as_slice(),
            ),
            (
                items[1],
                Role::ArmorWorld { sex: Sex::Male },
                b"male_world.nif".as_slice(),
            ),
            (
                items[2],
                Role::ArmorBiped { sex: Sex::Male },
                b"male_biped.nif".as_slice(),
            ),
            (
                items[3],
                Role::WeaponModel { mod_mask: 0 },
                b"weapon_0.nif".as_slice(),
            ),
            (
                items[4],
                Role::WeaponFirstPerson { mod_mask: 1 },
                b"first_1.nif".as_slice(),
            ),
        ] {
            let requests = Requests::prepare(
                &world,
                content,
                choice(&world, owner, item, role),
                Default::default(),
            )
            .unwrap();
            let joined = requests
                .observe(&world, content, store, actors, assets, Default::default())
                .unwrap();
            let lot = joined.selection().selected_lot();
            assert_eq!(lot.id(), item);
            assert_eq!(lot.owner(), owner);
            let index = items.iter().position(|id| *id == item).unwrap();
            assert_eq!(
                lot.facts(),
                &facts(index, if index < 3 { 0x200 } else { 0x210 })
            );
            assert_eq!(joined.model().explicit_choice.equipment, lot.facts().base);
            assert_eq!(joined.model().explicit_choice.role, role);
            assert_eq!(joined.model().requests.len(), 1);
            assert_eq!(joined.model().requests[0].path.raw, expected_path);
            let json = serde_json::to_value(&joined).unwrap();
            assert_eq!(json["equipped_state_verified"], false);
            assert_eq!(json["actor_reference_bound"], false);
            assert_eq!(
                json["selection"]["active_mod_mask"],
                serde_json::Value::Null
            );
            assert_eq!(json["selection"]["equip_rules_supported"], false);
        }
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn cold_same_campaign_and_foreign_campaign_refuse_retained_intent_but_fresh_cold_request_matches() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |store, scripts, content, actors, assets| {
        let mut live = world(scripts);
        let (owner, items) = lots(&mut live);
        let role = Role::WeaponFirstPerson { mod_mask: 1 };
        let prepared = Requests::prepare(
            &live,
            content,
            choice(&live, owner, items[4], role),
            Default::default(),
        )
        .unwrap();
        let expected = serde_json::to_value(
            prepared
                .observe(&live, content, store, actors, assets, Default::default())
                .unwrap(),
        )
        .unwrap();
        let before = live.snapshot();
        let bytes = before
            .encode(WorldLimits::default().max_snapshot_bytes)
            .unwrap();
        let cold = World::restore(
            scripts,
            Snapshot::decode(&bytes, WorldLimits::default()).unwrap(),
            WorldLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            prepared.observe(&cold, content, store, actors, assets, Default::default()),
            Err(Error::State(fallout_runtime::Error::StaleHandle))
        ));
        let fresh = Requests::prepare(
            &cold,
            content,
            choice(&cold, owner, items[4], role),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(
                fresh
                    .observe(&cold, content, store, actors, assets, Default::default())
                    .unwrap()
            )
            .unwrap(),
            expected
        );
        let mut foreign = World::with_campaign(
            scripts,
            WorldLimits::default(),
            CampaignId::from_bytes([0x28; 16]).unwrap(),
        )
        .unwrap();
        let (_, other_items) = lots(&mut foreign);
        assert_eq!(items, other_items);
        assert!(matches!(
            prepared.observe(&foreign, content, store, actors, assets, Default::default()),
            Err(Error::ContextChanged)
        ));
        assert_eq!(cold.snapshot(), before);
        assert_eq!(live.snapshot(), before);
    });
}

#[test]
fn removed_transferred_changed_facts_wrong_owner_and_unknown_banks_refuse_before_model_escape() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |store, scripts, content, actors, assets| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let other = world.register_reference(None).unwrap();
        let role = Role::ArmorBiped { sex: Sex::Male };
        assert!(
            Requests::prepare(
                &world,
                content,
                choice(&world, other, items[0], role),
                Default::default()
            )
            .is_err()
        );
        world.initialize_inventory(other).unwrap();
        assert!(
            Requests::prepare(
                &world,
                content,
                choice(&world, other, items[0], role),
                Default::default()
            )
            .is_err()
        );
        world
            .add_item(
                other,
                Facts::unknown(form(0x200)),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            Requests::prepare(
                &world,
                content,
                choice(&world, other, items[0], role),
                Default::default()
            ),
            Err(Error::Selection(
                fallout_runtime::actor_rules::equipment::Error::WrongOwner
            ))
        ));
        let removed = Requests::prepare(
            &world,
            content,
            choice(&world, owner, items[0], role),
            Default::default(),
        )
        .unwrap();
        world
            .remove_item_quantity(items[0], NonZeroU32::new(2).unwrap())
            .unwrap();
        assert!(matches!(
            removed.observe(&world, content, store, actors, assets, Default::default()),
            Err(Error::State(fallout_runtime::Error::Invalid(_)))
        ));
        let moved = Requests::prepare(
            &world,
            content,
            choice(&world, owner, items[1], role),
            Default::default(),
        )
        .unwrap();
        world.transfer_item(items[1], other).unwrap();
        assert!(matches!(
            moved.observe(&world, content, store, actors, assets, Default::default()),
            Err(Error::RevisionChanged)
        ));
        let changed = Requests::prepare(
            &world,
            content,
            choice(&world, owner, items[2], role),
            Default::default(),
        )
        .unwrap();
        world
            .replace_item_facts(items[2], Facts::unknown(form(0x210)))
            .unwrap();
        assert!(matches!(
            changed.observe(&world, content, store, actors, assets, Default::default()),
            Err(Error::RevisionChanged)
        ));
        let before = world.snapshot();
        let new = Requests::prepare(
            &world,
            content,
            choice(&world, owner, items[2], Role::WeaponModel { mod_mask: 0 }),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            new.observe(&world, content, store, actors, assets, Default::default())
                .unwrap()
                .model()
                .explicit_choice
                .equipment,
            form(0x210)
        );
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn changed_source_body_same_headers_and_changed_actor_catalogue_or_content_refuse() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let altered = tempfile::tempdir().unwrap();
    fixture(altered.path(), 1);
    with_sources(temp.path(), |store, scripts, content, actors, assets| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let request = Requests::prepare(
            &world,
            content,
            choice(&world, owner, items[0], Role::ArmorBiped { sex: Sex::Male }),
            Default::default(),
        )
        .unwrap();
        let before = world.snapshot();
        with_sources(
            altered.path(),
            |changed_store, _, changed_content, changed_actors, changed_assets| {
                assert!(matches!(
                    request.observe(
                        &world,
                        content,
                        changed_store,
                        actors,
                        assets,
                        Default::default()
                    ),
                    Err(Error::Source(_))
                ));
                assert!(matches!(
                    request.observe(
                        &world,
                        content,
                        store,
                        changed_actors,
                        changed_assets,
                        Default::default()
                    ),
                    Err(Error::ContextChanged)
                ));
                assert!(matches!(
                    request.observe(
                        &world,
                        changed_content,
                        store,
                        actors,
                        assets,
                        Default::default()
                    ),
                    Err(Error::Content(_))
                ));
            },
        );
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn unavailable_base_actor_kind_invalid_role_and_wrong_equipment_kind_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |store, scripts, content, actors, assets| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let role = Role::ArmorBiped { sex: Sex::Male };
        for actor in [0x101, 0x999, 0x200] {
            let mut supplied = choice(&world, owner, items[0], role);
            supplied.actor = form(actor);
            assert!(Requests::prepare(&world, content, supplied, Default::default()).is_err());
        }
        for base in [0x201, 0x999] {
            let id = world
                .add_item(
                    owner,
                    Facts::unknown(form(base)),
                    NonZeroU32::new(1).unwrap(),
                )
                .unwrap();
            assert!(
                Requests::prepare(
                    &world,
                    content,
                    choice(&world, owner, id, role),
                    Default::default()
                )
                .is_err()
            );
        }
        let request = Requests::prepare(
            &world,
            content,
            choice(&world, owner, items[5], role),
            Default::default(),
        )
        .unwrap();
        let observed = request
            .observe(&world, content, store, actors, assets, Default::default())
            .unwrap();
        assert!(observed.model().requests.is_empty());
        assert_eq!(
            observed.model().issues[0].code,
            "selected_equipment_wrong_kind"
        );
        let invalid = Requests::prepare(
            &world,
            content,
            choice(&world, owner, items[3], Role::WeaponModel { mod_mask: 8 }),
            Default::default(),
        )
        .unwrap();
        assert!(matches!(
            invalid.observe(&world, content, store, actors, assets, Default::default()),
            Err(Error::Source(_))
        ));
    });
}

#[test]
fn joined_projection_source_inventory_and_model_budget_edges_refuse_without_state_change() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |store, scripts, content, actors, assets| {
        let mut world = world(scripts);
        let (owner, items) = lots(&mut world);
        let before = world.snapshot();
        let request = Requests::prepare(
            &world,
            content,
            choice(
                &world,
                owner,
                items[4],
                Role::WeaponFirstPerson { mod_mask: 1 },
            ),
            Default::default(),
        )
        .unwrap();
        let result = request
            .observe(&world, content, store, actors, assets, Default::default())
            .unwrap();
        let exact = Limits {
            max_sources: 1,
            max_projection_bytes: serde_json::to_vec(&result).unwrap().len(),
            ..Default::default()
        };
        assert!(
            request
                .observe(&world, content, store, actors, assets, exact)
                .is_ok()
        );
        for label in [
            "projection",
            "source",
            "inventory",
            "selection visits",
            "model bytes",
            "model fields",
            "model requests",
            "model visits",
            "model source",
        ] {
            let mut under = exact;
            match label {
                "projection" => under.max_projection_bytes -= 1,
                "source" => under.max_sources = 0,
                "inventory" => under.selection.inventory.max_items = 5,
                "selection visits" => under.selection.max_visits = 7,
                "model bytes" => {
                    under.model.max_decoded_bytes = result.model().counts.decoded_bytes - 1
                }
                "model fields" => under.model.max_fields = result.model().counts.fields - 1,
                "model requests" => under.model.max_requests = 0,
                "model visits" => under.model.max_visits = result.model().counts.visits - 1,
                "model source" => under.model.max_sources = 1,
                _ => unreachable!(),
            }
            assert!(
                request
                    .observe(&world, content, store, actors, assets, under)
                    .is_err(),
                "{label}"
            );
        }
        let zero = Limits {
            max_sources: 0,
            ..Default::default()
        };
        assert!(matches!(
            Requests::prepare(
                &world,
                content,
                choice(&world, owner, items[0], Role::ArmorBiped { sex: Sex::Male }),
                zero
            ),
            Err(Error::Capacity("source"))
        ));
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn export_authored_cold_join_for_independent_actual_cli_when_requested() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_EQUIPMENT_RENDER_EVIDENCE_DIR") else {
        return;
    };
    let path = std::path::PathBuf::from(destination).join("fixture");
    assert!(!path.exists());
    fixture(&path, 0);
    with_sources(&path, |store, scripts, content, actors, assets| {
        let mut live = world(scripts);
        let (owner, items) = lots(&mut live);
        let unknown = live.register_reference(None).unwrap();
        let empty = live.register_reference(None).unwrap();
        live.initialize_inventory(empty).unwrap();
        let other = live.register_reference(None).unwrap();
        live.initialize_inventory(other).unwrap();
        let other_item = live
            .add_item(
                other,
                Facts::unknown(form(0x200)),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap();
        let snapshot = live.snapshot();
        fs::write(
            path.join("snapshot.json"),
            snapshot
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap(),
        )
        .unwrap();
        let cold = World::restore(scripts, snapshot.clone(), WorldLimits::default()).unwrap();
        let cases = [
            (
                0,
                "armor-female-biped",
                Role::ArmorBiped { sex: Sex::Female },
            ),
            (1, "armor-male-world", Role::ArmorWorld { sex: Sex::Male }),
            (2, "armor-male-biped", Role::ArmorBiped { sex: Sex::Male }),
            (3, "weapon-model:0", Role::WeaponModel { mod_mask: 0 }),
            (3, "weapon-model:1", Role::WeaponModel { mod_mask: 1 }),
            (
                4,
                "weapon-first-person:0",
                Role::WeaponFirstPerson { mod_mask: 0 },
            ),
            (
                4,
                "weapon-first-person:1",
                Role::WeaponFirstPerson { mod_mask: 1 },
            ),
            (4, "weapon-shell", Role::WeaponShell),
            (4, "weapon-scope", Role::WeaponScope),
            (4, "weapon-world", Role::WeaponWorld),
            (5, "armor-male-biped", Role::ArmorBiped { sex: Sex::Male }),
        ];
        let mut requests = Vec::new();
        for (index, name, role) in cases {
            let request = Requests::prepare(
                &cold,
                content,
                choice(&cold, owner, items[index], role),
                Default::default(),
            )
            .unwrap();
            let joined = request
                .observe(&cold, content, store, actors, assets, Default::default())
                .unwrap();
            let filename = format!(
                "host-item-{}-{}.json",
                items[index].0,
                name.replace(':', "_")
            );
            fs::write(path.join(&filename), serde_json::to_vec(&joined).unwrap()).unwrap();
            requests.push(serde_json::json!({"item":items[index],"role":name,"host":filename}));
        }
        fs::write(path.join("requests.json"), serde_json::to_vec(&serde_json::json!({"owner":owner,"items":items,"cases":requests,"unknown":unknown,"empty":empty,"other":other,"other_item":other_item,"missing_item":ItemId(NonZeroU64::new(999).unwrap())})).unwrap()).unwrap();
        assert_eq!(cold.snapshot(), snapshot);
    });
}
