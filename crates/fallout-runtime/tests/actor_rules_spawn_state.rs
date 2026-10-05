mod common;
use common::*;
use fallout_data::{
    actors::{self, associations, classes, dependencies, placements, races},
    inventory, leveled, loaded_scripts, plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::spawn_state::{self, Error, ItemResolution},
    foreign::Content,
};
use std::{fs, path::Path};

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut raw = record(kind, id, flags, body);
    raw[20..22].copy_from_slice(&15_u16.to_le_bytes());
    raw
}

fn cnto(base: u32, count: i32) -> Vec<u8> {
    field(
        b"CNTO",
        &[base.to_le_bytes().as_slice(), &count.to_le_bytes()].concat(),
    )
}

fn actor() -> Vec<u8> {
    let mut configuration = [0; 24];
    configuration[22..24].copy_from_slice(&0_u16.to_le_bytes());
    [
        field(b"ACBS", &configuration),
        field(
            b"DATA",
            &[12_i32.to_le_bytes().as_slice(), &[1, 2, 3, 4, 5, 6, 7]].concat(),
        ),
        field(b"DNAM", &[0; 28]),
        field(b"RNAM", &0x300_u32.to_le_bytes()),
        field(b"CNAM", &0x400_u32.to_le_bytes()),
        cnto(0x500, 2),
        field(
            b"COED",
            &[
                0_u32.to_le_bytes().as_slice(),
                u32::MAX.to_le_bytes().as_slice(),
                0x3f80_0000_u32.to_le_bytes().as_slice(),
            ]
            .concat(),
        ),
        cnto(0x501, -1),
        cnto(0x999, 0),
    ]
    .concat()
}

fn placed(base: u32) -> Vec<u8> {
    let words = [
        0_f32.to_bits(),
        1_f32.to_bits(),
        2_f32.to_bits(),
        0_f32.to_bits(),
        0_f32.to_bits(),
        0_f32.to_bits(),
    ];
    [
        field(b"EDID", b"PlacedActor\0"),
        field(b"NAME", &base.to_le_bytes()),
        field(
            b"DATA",
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
    ]
    .concat()
}

fn fixture(path: &Path) {
    let race = [
        field(b"DATA", &[0; 36]),
        field(b"PNAM", &[0; 4]),
        field(b"UNAM", &[0; 4]),
    ]
    .concat();
    let class = [field(b"DATA", &[0; 28]), field(b"ATTR", &[0; 7])].concat();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, 0, &actor()),
            disk(b"RACE", 0x300, 0, &race),
            disk(b"CLAS", 0x400, 0, &class),
            disk(b"ARMO", 0x500, 0, &field(b"MODL", b"armor.nif\0")),
            disk(b"LVLI", 0x501, 0, &field(b"UNKN", b"opaque list")),
            disk(b"ACHR", 0x600, 0, &placed(0x100)),
        ]
        .concat(),
    )
    .unwrap();
}

#[test]
fn placed_spawn_inputs_join_existing_sources_without_applying_them() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path());
    let mut store = RecordStore::open_nv_headers(
        temp.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 256).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let placements = placements::Catalogue::load(&mut store, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let races = races::Catalogue::load(&mut store, Default::default()).unwrap();
    let classes = classes::Catalogue::load(&mut store, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut store, Default::default()).unwrap();
    let dependencies = dependencies::Catalogue::load(
        &mut store,
        &actors,
        &associations,
        &lists,
        Default::default(),
    )
    .unwrap();
    let mut world = World::new(&scripts, WorldLimits::default()).unwrap();
    let reference = world.register_reference(Some(form(0x600))).unwrap();
    let before = world.snapshot();

    let requests = spawn_state::Requests::prepare(
        &mut store,
        &world,
        &content,
        &placements,
        &actors,
        &associations,
        &races,
        &classes,
        &dependencies,
        reference,
        Default::default(),
    )
    .unwrap();
    let observed = requests
        .observe(&world, &content, Default::default())
        .unwrap();

    assert!(observed.actor_reference_bound);
    assert_eq!(observed.reference_context.actor.key, &form(0x100));
    assert_eq!(observed.initialization_inputs.manifest.links().len(), 2);
    assert!(!observed.actor_initialization_supported);
    assert!(!observed.inventory_initialization_supported);
    assert!(!observed.equipment_selection_supported);
    assert!(observed.current_actor_values.is_none());
    assert!(observed.statistics.current_actor_values.is_none());
    assert!(observed.statistics.missing_fields.is_empty());
    assert_eq!(observed.inventory_items.len(), 3);
    assert!(matches!(
        observed.inventory_items[0].resolution,
        ItemResolution::DirectItemDefinition
    ));
    assert_eq!(observed.inventory_items[0].signed_source_count, 2);
    assert_eq!(observed.inventory_items[0].coed_fields.len(), 1);
    assert_eq!(
        observed.inventory_items[0].equipment_candidate_kind,
        Some(*b"ARMO")
    );
    assert!(matches!(
        observed.inventory_items[1].resolution,
        ItemResolution::LeveledSelectionRequired { target_kind } if target_kind == *b"LVLI"
    ));
    assert_eq!(observed.inventory_items[1].signed_source_count, -1);
    assert_eq!(observed.inventory_items[1].equipment_candidate_kind, None);
    assert!(matches!(
        observed.inventory_items[2].resolution,
        ItemResolution::Unavailable {
            reason: "missing_item_base"
        }
    ));
    assert_eq!(observed.inventory_items[2].signed_source_count, 0);
    assert_eq!(world.snapshot(), before);

    assert!(matches!(
        spawn_state::Requests::prepare(
            &mut store,
            &world,
            &content,
            &placements,
            &actors,
            &associations,
            &races,
            &classes,
            &dependencies,
            reference,
            spawn_state::Limits {
                max_inventory_items: 1,
                ..Default::default()
            },
        ),
        Err(Error::Capacity("inventory item"))
    ));
    assert_eq!(world.snapshot(), before);

    assert!(matches!(
        spawn_state::Requests::prepare(
            &mut store,
            &world,
            &content,
            &placements,
            &actors,
            &associations,
            &races,
            &classes,
            &dependencies,
            reference,
            spawn_state::Limits {
                max_extra_fields: 0,
                ..Default::default()
            },
        ),
        Err(Error::Capacity("extra field"))
    ));
    assert_eq!(world.snapshot(), before);
}
