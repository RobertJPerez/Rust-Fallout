#[path = "actor_rules_reference_intent_fixture.rs"]
mod fixture;
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::{
        inventory_transfer::{self, Choice},
        reference_intent::{self, Error, Limits},
    },
    inventory::Facts,
};
use fixture::*;
use std::num::NonZeroU32;

#[test]
fn transfer_exact_same_base_lot_without_merge_retains_every_fact_and_cold_state() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let live = world(scripts);
        let before = live.snapshot();
        for selected in [1, 2, 3] {
            let request = Choice {
                claim: claim(&before, 1, 0x100),
                item: item(selected),
                destination: reference(3),
            };
            let candidate = inventory_transfer::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                Limits::default(),
            )
            .unwrap();
            let cold = World::restore(
                scripts,
                candidate.snapshot().clone(),
                WorldLimits::default(),
            )
            .unwrap();
            let original = live.item(item(selected)).unwrap();
            let moved = cold.item(item(selected)).unwrap();
            assert_eq!(moved.owner(), reference(3));
            assert_eq!(moved.id(), original.id());
            assert_eq!(moved.facts(), original.facts());
            assert_eq!(moved.count(), original.count());
            for id in [1, 2, 3, 4] {
                if id != selected {
                    assert_eq!(cold.item(item(id)).unwrap(), live.item(item(id)).unwrap());
                }
            }
            assert_eq!(cold.inventory_items(reference(3)).unwrap().count(), 2);
            conserve(&before, candidate.snapshot(), false, true, 1);
            assert_cold(scripts, candidate.snapshot());
            let json = serde_json::to_value(&candidate).unwrap();
            assert_eq!(json["operation"]["changed"], true);
            assert_eq!(live.snapshot(), before);
        }
    });
}
#[test]
fn same_owner_is_a_kernel_noop_and_empty_destination_is_explicitly_admitted() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let before = world(scripts).snapshot();
        for target in [1, 2] {
            let request = Choice {
                claim: claim(&before, 1, 0x100),
                item: item(2),
                destination: reference(target),
            };
            let candidate = inventory_transfer::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                Limits::default(),
            )
            .unwrap();
            if target == 1 {
                assert_eq!(candidate.snapshot(), &before);
                assert_eq!(candidate.input_sha256(), candidate.candidate_sha256());
            } else {
                conserve(&before, candidate.snapshot(), false, true, 1);
                let cold = World::restore(
                    scripts,
                    candidate.snapshot().clone(),
                    WorldLimits::default(),
                )
                .unwrap();
                assert_eq!(cold.inventory_items(reference(2)).unwrap().count(), 1);
            }
            assert_cold(scripts, candidate.snapshot());
        }
        let maxed = fallout_runtime::snapshot::Snapshot {
            state_revision: u64::MAX,
            ..before.clone()
        };
        let noop = Choice {
            claim: claim(&maxed, 1, 0x100),
            item: item(2),
            destination: reference(1),
        };
        assert_eq!(
            inventory_transfer::apply_private(
                &maxed,
                scripts,
                content,
                sources,
                &noop,
                WorldLimits::default(),
                Limits::default()
            )
            .unwrap()
            .snapshot(),
            &maxed
        );
        let change = Choice {
            destination: reference(2),
            ..noop
        };
        assert!(
            inventory_transfer::apply_private(
                &maxed,
                scripts,
                content,
                sources,
                &change,
                WorldLimits::default(),
                Limits::default()
            )
            .is_err()
        );
    });
}
#[test]
fn unknown_bank_owner_missing_lot_and_source_refuse_without_creating_a_destination() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let mut live = world(scripts);
        let bad_source = live
            .add_item(
                reference(1),
                Facts::unknown(form(0x301)),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap();
        let before = live.snapshot();
        let good = Choice {
            claim: claim(&before, 1, 0x100),
            item: item(1),
            destination: reference(2),
        };
        let mut bads = Vec::new();
        for destination in [reference(4), reference(999)] {
            bads.push(Choice {
                destination,
                ..good.clone()
            });
        }
        for id in [item(4), item(999), bad_source] {
            bads.push(Choice {
                item: id,
                ..good.clone()
            });
        }
        let mut bad = good.clone();
        bad.claim.actor = form(0x200);
        bads.push(bad);
        let mut bad = good.clone();
        bad.claim.expected_snapshot_sha256 = "00".repeat(32);
        bads.push(bad);
        let mut bad = good.clone();
        bad.claim.intent = reference_intent::Intent::Faithful {};
        bads.push(bad);
        let empty = Choice {
            claim: claim(&before, 2, 0x200),
            item: item(1),
            destination: reference(1),
        };
        assert!(matches!(
            inventory_transfer::apply_private(
                &before,
                scripts,
                content,
                sources,
                &empty,
                WorldLimits::default(),
                Limits::default()
            ),
            Err(Error::Equipment(
                fallout_runtime::actor_rules::equipment::Error::InventoryEmpty
            ))
        ));
        for bad in bads {
            assert!(
                inventory_transfer::apply_private(
                    &before,
                    scripts,
                    content,
                    sources,
                    &bad,
                    WorldLimits::default(),
                    Limits::default()
                )
                .is_err()
            );
            assert_eq!(live.snapshot(), before);
        }
        let mut no_bank = before.clone();
        no_bank.inventory_banks.retain(|b| b.owner != reference(1));
        let req = Choice {
            claim: claim(&no_bank, 1, 0x100),
            ..good.clone()
        };
        assert!(matches!(
            inventory_transfer::apply_private(
                &no_bank,
                scripts,
                content,
                sources,
                &req,
                WorldLimits::default(),
                Limits::default()
            ),
            Err(Error::Equipment(
                fallout_runtime::actor_rules::equipment::Error::InventoryUninitialized
            ))
        ));
        live.remove_item_quantity(item(1), NonZeroU32::new(2).unwrap())
            .unwrap();
        let removed = live.snapshot();
        let req = Choice {
            claim: claim(&removed, 1, 0x100),
            ..good
        };
        assert!(matches!(
            inventory_transfer::apply_private(
                &removed,
                scripts,
                content,
                sources,
                &req,
                WorldLimits::default(),
                Limits::default()
            ),
            Err(Error::Equipment(
                fallout_runtime::actor_rules::equipment::Error::ItemUnavailable
            ))
        ));
    });
}
#[test]
fn exact_projection_and_nested_inventory_budget_refusal_drops_candidate() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let before = world(scripts).snapshot();
        let request = Choice {
            claim: claim(&before, 1, 0x100),
            item: item(2),
            destination: reference(2),
        };
        let candidate = inventory_transfer::apply_private(
            &before,
            scripts,
            content,
            sources,
            &request,
            WorldLimits::default(),
            Limits::default(),
        )
        .unwrap();
        let exact = Limits {
            max_projection_bytes: serde_json::to_vec(&candidate).unwrap().len(),
            max_visits: candidate.visits(),
            ..Limits::default()
        };
        assert!(
            inventory_transfer::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                exact
            )
            .is_ok()
        );
        let under = Limits {
            max_projection_bytes: exact.max_projection_bytes - 1,
            ..exact
        };
        assert!(matches!(
            inventory_transfer::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                under
            ),
            Err(Error::Capacity("projection byte"))
        ));
        let mut under = exact;
        under.equipment.inventory.max_items = 2;
        assert!(
            inventory_transfer::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                under
            )
            .is_err()
        );
        let mut under = exact;
        under.max_visits -= 1;
        assert!(matches!(
            inventory_transfer::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                under
            ),
            Err(Error::Capacity("visit"))
        ));
    });
}
