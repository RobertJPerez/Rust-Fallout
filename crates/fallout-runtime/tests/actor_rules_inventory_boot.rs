mod common;
use common::*;
use fallout_data::{actors, inventory, loaded_scripts::Catalogue, plugin, store::RecordStore};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::inventory_boot::{self, Choice, Error, Limits},
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    inventory::{Condition, Facts, OpaqueExtra},
    snapshot::Snapshot,
};
use std::{
    fs,
    num::{NonZeroU32, NonZeroU64},
    path::Path,
};

fn reference_id(value: u64) -> ReferenceId {
    ReferenceId(NonZeroU64::new(value).unwrap())
}

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = record(kind, id, flags, body);
    bytes[20..22].copy_from_slice(&15u16.to_le_bytes());
    bytes
}
fn cnto(base: u32, count: i32) -> Vec<u8> {
    field(
        b"CNTO",
        &[base.to_le_bytes().as_slice(), &count.to_le_bytes()].concat(),
    )
}
fn actor_body(creature: bool, marker: u8) -> Vec<u8> {
    let mut acbs = [0; 24];
    acbs[22..24].copy_from_slice(&256u16.to_le_bytes());
    let data = vec![marker; if creature { 17 } else { 11 }];
    [
        field(b"ACBS", &acbs),
        field(b"DATA", &data),
        cnto(0x200, 4),
        field(
            b"COED",
            &[
                0u32.to_le_bytes().as_slice(),
                &u32::MAX.to_le_bytes(),
                &0x7FC00031u32.to_le_bytes(),
            ]
            .concat(),
        ),
        cnto(0x200, -2),
        field(
            b"COED",
            &[
                0u32.to_le_bytes().as_slice(),
                &0x80000000u32.to_le_bytes(),
                &0x80000000u32.to_le_bytes(),
            ]
            .concat(),
        ),
        cnto(0x201, 1),
        cnto(0x202, 1),
        cnto(0x999, 1),
        cnto(0x203, 1),
        cnto(0, 1),
    ]
    .concat()
}
fn fixture(path: &Path, marker: u8) {
    fs::create_dir_all(path.join("Data")).unwrap();
    fs::write(
        path.join("Data/FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, 0, &actor_body(false, marker)),
            disk(b"CREA", 0x101, 0, &actor_body(true, marker)),
            disk(
                b"NPC_",
                0x103,
                plugin::DELETED,
                &field(b"UNKN", b"opaque tombstone"),
            ),
            disk(b"ARMO", 0x200, 0, &field(b"MODL", b"caller.nif\0")),
            disk(b"LVLI", 0x201, 0, &field(b"UNKN", b"unselected list")),
            disk(
                b"ARMO",
                0x202,
                plugin::DELETED,
                &field(b"UNKN", b"opaque deleted item"),
            ),
            disk(b"FACT", 0x203, 0, &[]),
            disk(b"IMOD", 0x300, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(path.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
}
fn with_source(path: &Path, callback: impl FnOnce(&Catalogue, &Content, &actors::Catalogue<'_>)) {
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
    callback(&scripts, &content, &actors);
}
fn world(scripts: &Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x18; 16]).unwrap(),
    )
    .unwrap();
    world.register_reference(None).unwrap();
    world
}
fn choices() -> Vec<Choice> {
    let mut first = Facts::unknown(form(0x200));
    first.condition = Some(Condition::Float32 { bits: 0x80000000 });
    let mut second = Facts::unknown(form(0x200));
    second.equipped_slots = Some(vec![]);
    second.modifications = Some(vec![form(0x300), form(0x300)]);
    second.extra_fields = vec![OpaqueExtra {
        tag: *b"ZZZZ",
        bytes: vec![0, 255, 1],
    }];
    vec![
        Choice {
            field_index: 2,
            host_count: NonZeroU32::new(5).unwrap(),
            facts: first,
            source_count_claim: Some(4),
        },
        Choice {
            field_index: 4,
            host_count: NonZeroU32::new(7).unwrap(),
            facts: second,
            source_count_claim: Some(-2),
        },
    ]
}

#[test]
fn private_candidate_preserves_duplicate_physical_entries_explicit_facts_and_cold_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 0);
    with_source(dir.path(), |scripts, content, actors| {
        let world = world(scripts);
        let before = world.snapshot();
        let choices = choices();
        for root in [0x100, 0x101] {
            let plan = inventory_boot::prepare(
                &world,
                content,
                actors,
                &form(root),
                reference_id(1),
                &choices,
                Limits::default(),
            )
            .unwrap();
            let boot = plan
                .apply_private(scripts, content, &before, WorldLimits::default())
                .unwrap();
            assert_eq!(
                boot.mappings
                    .iter()
                    .map(|m| (
                        m.source_field_index,
                        m.source_signed_count,
                        m.explicit_host_count,
                        m.item.0.get()
                    ))
                    .collect::<Vec<_>>(),
                [(2, 4, 5, 1), (4, -2, 7, 2)]
            );
            assert!(!boot.faithful_initialization_supported);
            assert_eq!(
                boot.candidate_snapshot.state_revision,
                before.state_revision + 3
            );
            let bytes = serde_json::to_vec(&boot.candidate_snapshot).unwrap();
            let snapshot = Snapshot::decode(&bytes, WorldLimits::default()).unwrap();
            let cold = World::restore(scripts, snapshot, WorldLimits::default()).unwrap();
            let lots = cold
                .inventory_items(reference_id(1))
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(lots.len(), 2);
            assert_eq!(lots[0].facts(), &choices[0].facts);
            assert_eq!(lots[1].facts(), &choices[1].facts);
            assert_eq!(lots[0].count(), 5);
            assert_eq!(lots[1].count(), 7);
            assert_eq!(cold.snapshot(), boot.candidate_snapshot);
            assert_eq!(world.snapshot(), before);
            assert_eq!(before.inventory_banks.len(), 0);
            let source_count = match &boot.source_definition.fields[4].value {
                inventory::Value::Item { count, .. } => *count,
                _ => panic!(),
            };
            assert_eq!(source_count, -2);
        }
    });
}
#[test]
fn late_capacity_failure_drops_partial_candidate_and_preserves_input_allocator_and_inventory() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 0);
    with_source(dir.path(), |scripts, content, actors| {
        let world = world(scripts);
        let before = world.snapshot();
        let choices = choices();
        let plan = inventory_boot::prepare(
            &world,
            content,
            actors,
            &form(0x100),
            reference_id(1),
            &choices,
            Limits::default(),
        )
        .unwrap();
        assert!(matches!(
            plan.apply_private(
                scripts,
                content,
                &before,
                WorldLimits {
                    max_item_instances: 1,
                    ..Default::default()
                }
            ),
            Err(Error::State(fallout_runtime::Error::Capacity(
                "item instances"
            )))
        ));
        assert_eq!(world.snapshot(), before);
        assert_eq!(before.next_item, 1);
        assert!(before.inventory_banks.is_empty());
        let boot = inventory_boot::prepare(
            &world,
            content,
            actors,
            &form(0x100),
            reference_id(1),
            &choices,
            Limits::default(),
        )
        .unwrap()
        .apply_private(scripts, content, &before, WorldLimits::default())
        .unwrap();
        assert_eq!(boot.mappings[0].item.0.get(), 1);
        assert_eq!(boot.mappings[1].item.0.get(), 2);
    });
}
#[test]
fn claim_base_duplicate_or_wrong_field_and_unknown_owner_refuse_before_candidate() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 0);
    with_source(dir.path(), |scripts, content, actors| {
        let world = world(scripts);
        let before = world.snapshot();
        let mut wrong_count = choices();
        wrong_count[1].source_count_claim = Some(2);
        let mut wrong_base = choices();
        wrong_base[1].facts.base = form(0x300);
        let mut repeated = choices();
        repeated[1].field_index = 2;
        let mut not_item = choices();
        not_item[1].field_index = 3;
        for selection in [wrong_count, wrong_base, repeated, not_item, Vec::new()] {
            assert!(
                inventory_boot::prepare(
                    &world,
                    content,
                    actors,
                    &form(0x100),
                    reference_id(1),
                    &selection,
                    Limits::default()
                )
                .is_err()
            );
        }
        assert!(
            inventory_boot::prepare(
                &world,
                content,
                actors,
                &form(0x100),
                reference_id(999),
                &choices(),
                Limits::default()
            )
            .is_err()
        );
        assert_eq!(world.snapshot(), before);
        let mut unclaimed = choices();
        unclaimed[1].source_count_claim = None;
        assert!(
            inventory_boot::prepare(
                &world,
                content,
                actors,
                &form(0x100),
                reference_id(1),
                &unclaimed,
                Limits::default()
            )
            .is_ok()
        );
    });
}
#[test]
fn missing_deleted_leveled_wrong_kind_null_sources_and_initialized_empty_bank_refuse() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 0);
    with_source(dir.path(), |scripts, content, actors| {
        let mut world = world(scripts);
        let before = world.snapshot();
        for field_index in 6..=10 {
            let mut choice = choices()[0].clone();
            choice.field_index = field_index;
            choice.source_count_claim = None;
            choice.facts.base = form(match field_index {
                6 => 0x201,
                7 => 0x202,
                8 => 0x999,
                9 => 0x203,
                _ => 0x200,
            });
            assert!(
                inventory_boot::prepare(
                    &world,
                    content,
                    actors,
                    &form(0x100),
                    reference_id(1),
                    &[choice],
                    Limits::default()
                )
                .is_err()
            );
        }
        for id in [0x103, 0x999, 0x200] {
            assert!(
                inventory_boot::prepare(
                    &world,
                    content,
                    actors,
                    &form(id),
                    reference_id(1),
                    &choices(),
                    Limits::default()
                )
                .is_err()
            );
        }
        assert_eq!(world.snapshot(), before);
        world.initialize_inventory(reference_id(1)).unwrap();
        let initialized = world.snapshot();
        assert!(matches!(
            inventory_boot::prepare(
                &world,
                content,
                actors,
                &form(0x100),
                reference_id(1),
                &choices(),
                Limits::default()
            ),
            Err(Error::AlreadyInitialized)
        ));
        assert_eq!(world.snapshot(), initialized);
    });
}
#[test]
fn full_input_snapshot_pin_and_foreign_source_cohort_refuse_without_changing_either_world() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 0);
    let other = tempfile::tempdir().unwrap();
    fixture(other.path(), 1);
    with_source(dir.path(), |scripts, content, actors| {
        let world = world(scripts);
        let before = world.snapshot();
        let mut changed = before.clone();
        changed.campaign = CampaignId::from_bytes([0x19; 16]).unwrap();
        let plan = inventory_boot::prepare(
            &world,
            content,
            actors,
            &form(0x100),
            reference_id(1),
            &choices(),
            Limits::default(),
        )
        .unwrap();
        assert!(matches!(
            plan.apply_private(scripts, content, &changed, WorldLimits::default()),
            Err(Error::ContextChanged)
        ));
        with_source(other.path(), |_, foreign_content, foreign_actors| {
            assert!(
                inventory_boot::prepare(
                    &world,
                    foreign_content,
                    actors,
                    &form(0x100),
                    reference_id(1),
                    &choices(),
                    Limits::default()
                )
                .is_err()
            );
            assert!(matches!(
                inventory_boot::prepare(
                    &world,
                    content,
                    foreign_actors,
                    &form(0x100),
                    reference_id(1),
                    &choices(),
                    Limits::default()
                ),
                Err(Error::ContextChanged)
            ));
        });
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn every_bound_accepts_exact_extent_and_rejects_one_less_including_candidate_bytes() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 0);
    with_source(dir.path(), |scripts, content, actors| {
        let world = world(scripts);
        let before = world.snapshot();
        let choices = choices();
        let plan = inventory_boot::prepare(
            &world,
            content,
            actors,
            &form(0x100),
            reference_id(1),
            &choices,
            Limits::default(),
        )
        .unwrap();
        let plan_bytes = serde_json::to_vec(&plan).unwrap().len();
        let input_bytes = serde_json::to_vec(&before).unwrap().len();
        let boot = plan
            .apply_private(scripts, content, &before, WorldLimits::default())
            .unwrap();
        let output_bytes = serde_json::to_vec(&boot).unwrap().len();
        let candidate_bytes = serde_json::to_vec(&boot.candidate_snapshot).unwrap().len();
        let exact = Limits {
            max_sources: 1,
            max_field_visits: 20,
            max_lots: 2,
            max_fact_links: 3,
            max_extra_bytes: 3,
            max_snapshot_bytes: candidate_bytes,
            max_projection_bytes: output_bytes.max(plan_bytes),
        };
        assert!(
            inventory_boot::prepare(
                &world,
                content,
                actors,
                &form(0x100),
                reference_id(1),
                &choices,
                exact
            )
            .unwrap()
            .apply_private(scripts, content, &before, WorldLimits::default())
            .is_ok()
        );
        for bad in [
            Limits {
                max_sources: 0,
                ..exact
            },
            Limits {
                max_field_visits: 19,
                ..exact
            },
            Limits {
                max_lots: 1,
                ..exact
            },
            Limits {
                max_fact_links: 2,
                ..exact
            },
            Limits {
                max_extra_bytes: 2,
                ..exact
            },
            Limits {
                max_snapshot_bytes: input_bytes - 1,
                ..exact
            },
            Limits {
                max_projection_bytes: plan_bytes - 1,
                ..exact
            },
        ] {
            assert!(
                inventory_boot::prepare(
                    &world,
                    content,
                    actors,
                    &form(0x100),
                    reference_id(1),
                    &choices,
                    bad
                )
                .is_err()
            );
        }
        let plan = inventory_boot::prepare(
            &world,
            content,
            actors,
            &form(0x100),
            reference_id(1),
            &choices,
            Limits {
                max_snapshot_bytes: candidate_bytes - 1,
                ..exact
            },
        )
        .unwrap();
        assert!(matches!(
            plan.apply_private(scripts, content, &before, WorldLimits::default()),
            Err(Error::Capacity("candidate snapshot byte"))
        ));
        assert_eq!(world.snapshot(), before);
        if output_bytes > plan_bytes {
            let plan = inventory_boot::prepare(
                &world,
                content,
                actors,
                &form(0x100),
                reference_id(1),
                &choices,
                Limits {
                    max_projection_bytes: output_bytes - 1,
                    ..exact
                },
            )
            .unwrap();
            assert!(matches!(
                plan.apply_private(scripts, content, &before, WorldLimits::default()),
                Err(Error::Capacity("result projection byte"))
            ));
        }
    });
}
#[test]
fn engineering_request_decode_rejects_zero_count_and_unknown_claim_fields() {
    let mut request = serde_json::to_value(choices()).unwrap();
    request[0]["host_count"] = serde_json::json!(0);
    assert!(serde_json::from_value::<Vec<Choice>>(request).is_err());
    let mut request = serde_json::to_value(choices()).unwrap();
    request[0]["condition_claim"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Vec<Choice>>(request).is_err());
}
#[test]
fn export_private_boot_inputs_and_cold_host_when_requested() {
    let Ok(path) = std::env::var("FALLOUT_ACTOR_BOOT_EVIDENCE_DIR") else {
        return;
    };
    let path = Path::new(&path).join("fixture");
    fixture(&path, 0);
    with_source(&path, |scripts, content, actors| {
        let world = world(scripts);
        let before = world.snapshot();
        fs::write(
            path.join("snapshot.json"),
            serde_json::to_vec_pretty(&before).unwrap(),
        )
        .unwrap();
        fs::write(
            path.join("choices.json"),
            serde_json::to_vec_pretty(&choices()).unwrap(),
        )
        .unwrap();
        let boot = inventory_boot::prepare(
            &world,
            content,
            actors,
            &form(0x100),
            reference_id(1),
            &choices(),
            Limits::default(),
        )
        .unwrap()
        .apply_private(scripts, content, &before, WorldLimits::default())
        .unwrap();
        fs::write(
            path.join("host-boot.json"),
            serde_json::to_vec_pretty(&boot).unwrap(),
        )
        .unwrap();
        fs::write(
            path.join("candidate-snapshot.json"),
            serde_json::to_vec_pretty(&boot.candidate_snapshot).unwrap(),
        )
        .unwrap();
        let cold =
            World::restore(scripts, boot.candidate_snapshot, WorldLimits::default()).unwrap();
        fs::write(
            path.join("cold-snapshot.json"),
            serde_json::to_vec_pretty(&cold.snapshot()).unwrap(),
        )
        .unwrap();
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
#[ignore = "explicit private installed-source engineering fixture; run under heavy slot"]
fn export_explicit_installed_boot_owner_and_cold_candidate() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        data: std::path::PathBuf,
        load_order: Vec<String>,
        actor: fallout_data::identity::FormKey,
        snapshot: std::path::PathBuf,
        choices: Vec<Choice>,
        destination: std::path::PathBuf,
    }
    let request_path =
        std::env::var("FALLOUT_ACTOR_BOOT_INSTALLED_REQUEST").expect("explicit request required");
    let request: Request = serde_json::from_slice(&fs::read(request_path).unwrap()).unwrap();
    let mut store =
        RecordStore::open_nv_headers(&request.data, &request.load_order, Default::default())
            .unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 2_000_000).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let source_snapshot = Snapshot::decode(
        &fs::read(&request.snapshot).unwrap(),
        WorldLimits::default(),
    )
    .unwrap();
    let mut world = World::restore(&scripts, source_snapshot, WorldLimits::default()).unwrap();
    // One explicitly requested private engineering owner, without authored-origin
    // inference or any actor pose/enable/inventory initialization.
    let owner = world.register_reference(None).unwrap();
    let input = world.snapshot();
    fs::create_dir_all(&request.destination).unwrap();
    fs::write(
        request.destination.join("input-snapshot.json"),
        serde_json::to_vec_pretty(&input).unwrap(),
    )
    .unwrap();
    fs::write(
        request.destination.join("owner.json"),
        serde_json::to_vec(&owner).unwrap(),
    )
    .unwrap();
    let boot = inventory_boot::prepare(
        &world,
        &content,
        &actors,
        &request.actor,
        owner,
        &request.choices,
        Limits::default(),
    )
    .unwrap()
    .apply_private(&scripts, &content, &input, WorldLimits::default())
    .unwrap();
    fs::write(
        request.destination.join("host-boot.json"),
        serde_json::to_vec_pretty(&boot).unwrap(),
    )
    .unwrap();
    fs::write(
        request.destination.join("candidate-snapshot.json"),
        serde_json::to_vec_pretty(&boot.candidate_snapshot).unwrap(),
    )
    .unwrap();
    let cold = World::restore(&scripts, boot.candidate_snapshot, WorldLimits::default()).unwrap();
    fs::write(
        request.destination.join("cold-snapshot.json"),
        serde_json::to_vec_pretty(&cold.snapshot()).unwrap(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), input);
}
