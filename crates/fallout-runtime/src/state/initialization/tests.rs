use super::*;
use crate::{Limits as WorldLimits, save::Captured};
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore};
use std::fs;

fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(tag: &[u8; 4], id: u32, data: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}
fn unit(locals: &[u32]) -> Vec<u8> {
    let compiled = [0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0];
    let mut header = [0; 20];
    header[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    header[12..16].copy_from_slice(&(locals.len() as u32).to_le_bytes());
    let mut data = field(b"SCHR", &header);
    data.extend(field(b"SCDA", &compiled));
    for index in locals {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&index.to_le_bytes());
        data.extend(field(b"SLSD", &declaration));
        data.extend(field(b"SCVR", b"explicit\0"));
    }
    data
}
fn fixture(root: &std::path::Path) -> (Catalogue, Handle, Handle) {
    let header = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    fs::write(
        root.join("FalloutNV.esm"),
        [
            record(b"TES4", 0, &header),
            record(b"SCPT", 0x300, &unit(&[42])),
            record(b"SCPT", 0x301, &unit(&[42, 43])),
        ]
        .concat(),
    )
    .unwrap();
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let handles: Vec<_> = catalogue
        .iter()
        .map(|(_, script)| script.handle().clone())
        .collect();
    (catalogue, handles[0].clone(), handles[1].clone())
}
fn owner(n: u64) -> Owner {
    Owner::Fragment {
        activation: n.try_into().unwrap(),
    }
}

#[test]
fn private_preparation_drop_and_publication_have_exact_schema_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, definition, _) = fixture(temp.path());
    let mut world = World::new(&catalogue, WorldLimits::default()).unwrap();
    let before = world.snapshot();
    let stage = world
        .stage_instance_initialization(
            &definition,
            &owner(1),
            &Context::default(),
            &[],
            Limits::default(),
        )
        .unwrap();
    let discarded = Arc::downgrade(&stage.schema);
    assert_eq!(discarded.strong_count(), 1);
    assert!(world.definitions.is_empty());
    assert_eq!(world.block_count, 0);
    assert_eq!(world.snapshot(), before);
    drop(stage);
    assert!(discarded.upgrade().is_none());
    let stage = world
        .stage_instance_initialization(
            &definition,
            &owner(1),
            &Context::default(),
            &[],
            Limits::default(),
        )
        .unwrap();
    let retained = Arc::downgrade(&stage.schema);
    let (_, handle) = world.commit_instance_initialization(stage).unwrap();
    assert_eq!(retained.strong_count(), 2); // one cache, one running instance
    assert_eq!(world.block_count, 1);
    assert!(Arc::ptr_eq(
        &world.definitions[&definition.key],
        &world.instance(handle).unwrap().definition_schema
    ));
    let capture = Captured::at_boundary(&world);
    assert_eq!(retained.strong_count(), 3);
    drop(capture);
    assert_eq!(retained.strong_count(), 2);
    world.remove_instance(handle).unwrap();
    assert_eq!(retained.strong_count(), 1);
    drop(world);
    assert!(retained.upgrade().is_none());
}

#[test]
fn failed_legacy_create_warming_another_schema_cannot_evade_commit_block_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, first, other) = fixture(temp.path());
    let mut world = World::new(
        &catalogue,
        WorldLimits {
            max_locals: 1,
            max_event_blocks: 1,
            ..WorldLimits::default()
        },
    )
    .unwrap();
    let stage = world
        .stage_instance_initialization(
            &first,
            &owner(1),
            &Context::default(),
            &[],
            Limits::default(),
        )
        .unwrap();
    let proposed = Arc::downgrade(&stage.schema);
    let before = world.snapshot();
    assert!(matches!(
        world.create_instance(&other, owner(2), Context::default()),
        Err(Error::Capacity("local variables"))
    ));
    assert_eq!(world.snapshot(), before);
    assert_eq!(world.block_count, 1);
    assert!(world.definitions.contains_key(&other.key));
    assert!(!world.definitions.contains_key(&first.key));
    assert!(matches!(
        world.commit_instance_initialization(stage),
        Err(Error::Capacity("compiled event blocks"))
    ));
    assert_eq!(world.snapshot(), before);
    assert!(proposed.upgrade().is_none());
    assert_eq!(world.definitions.len(), 1);
    assert!(world.owners.is_empty());
    assert_eq!(world.block_count, 1);
}
