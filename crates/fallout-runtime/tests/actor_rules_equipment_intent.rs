#[path = "actor_rules_reference_intent_fixture.rs"]
mod fixture;
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::{
        equipment_intent::{self, Choice},
        inventory_transfer,
        reference_intent::{self, Error, Limits},
    },
};
use fixture::*;

#[test]
fn explicit_unknown_known_empty_unsorted_slots_preserve_all_other_facts_and_other_lots() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let live = world(scripts);
        let before = live.snapshot();
        for selected in [1, 2, 3] {
            for slots in [None, Some(vec![]), Some(vec![u16::MAX, 0, 7])] {
                let request = Choice {
                    claim: claim(&before, 1, 0x100),
                    item: item(selected),
                    supplied_slots: slots.clone(),
                };
                let candidate = equipment_intent::apply_private(
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
                let changed = cold.item(item(selected)).unwrap();
                let mut expected = original.facts().clone();
                expected.equipped_slots = slots;
                assert_eq!(changed.facts(), &expected);
                assert_eq!(changed.count(), original.count());
                assert_eq!(changed.owner(), original.owner());
                assert_eq!(changed.id(), original.id());
                for id in [1, 2, 3, 4] {
                    if id != selected {
                        assert_eq!(cold.item(item(id)).unwrap(), live.item(item(id)).unwrap());
                    }
                }
                conserve(&before, candidate.snapshot(), false, true, 1);
                assert_cold(scripts, candidate.snapshot());
                assert_eq!(live.snapshot(), before);
                let same = Choice {
                    claim: claim(candidate.snapshot(), 1, 0x100),
                    ..request
                };
                let again = equipment_intent::apply_private(
                    candidate.snapshot(),
                    scripts,
                    content,
                    sources,
                    &same,
                    WorldLimits::default(),
                    Limits::default(),
                )
                .unwrap();
                conserve(candidate.snapshot(), again.snapshot(), false, false, 1);
            }
        }
    });
}
#[test]
fn duplicates_wrong_lot_owner_claim_and_equal_value_revision_overflow_refuse() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let live = world(scripts);
        let before = live.snapshot();
        let good = Choice {
            claim: claim(&before, 1, 0x100),
            item: item(1),
            supplied_slots: None,
        };
        let mut bads = vec![
            Choice {
                supplied_slots: Some(vec![7, 7]),
                ..good.clone()
            },
            Choice {
                item: item(4),
                ..good.clone()
            },
            Choice {
                item: item(999),
                ..good.clone()
            },
        ];
        let mut bad = good.clone();
        bad.claim.actor = form(0x200);
        bads.push(bad);
        let mut bad = good.clone();
        bad.claim.expected_snapshot_sha256 = "00".repeat(32);
        bads.push(bad);
        let mut bad = good.clone();
        bad.claim.intent = reference_intent::Intent::Faithful {};
        bads.push(bad);
        for bad in bads {
            assert!(
                equipment_intent::apply_private(
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
        let mut maxed = before.clone();
        maxed.state_revision = u64::MAX;
        let request = Choice {
            claim: claim(&maxed, 1, 0x100),
            ..good
        };
        assert!(
            equipment_intent::apply_private(
                &maxed,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                Limits::default()
            )
            .is_err()
        );
    });
}
#[test]
fn required_nullable_slot_wire_refuses_omission_unknown_properties_and_duplicate_fields() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, _, _| {
        let before = world(scripts).snapshot();
        let request = Choice {
            claim: claim(&before, 1, 0x100),
            item: item(1),
            supplied_slots: None,
        };
        let original = serde_json::to_value(request).unwrap();
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove("supplied_slots");
        assert!(serde_json::from_value::<Choice>(missing).is_err());
        let mut bad = original.clone();
        bad["unknown"] = serde_json::json!(1);
        assert!(serde_json::from_value::<Choice>(bad).is_err());
        let mut bad = original.clone();
        bad["claim"]["intent"]["unknown"] = serde_json::json!(1);
        assert!(serde_json::from_value::<Choice>(bad).is_err());
        let duplicate = serde_json::to_string(&original).unwrap().replacen(
            "\"supplied_slots\":null",
            "\"supplied_slots\":null,\"supplied_slots\":[]",
            1,
        );
        assert!(serde_json::from_str::<Choice>(&duplicate).is_err());
        assert!(
            serde_json::from_value::<Choice>(original)
                .unwrap()
                .supplied_slots
                .is_none()
        );
    });
}
#[test]
fn exact_slot_fact_extra_work_output_and_canonical_limits_refuse_before_result() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let before = world(scripts).snapshot();
        let request = Choice {
            claim: claim(&before, 1, 0x100),
            item: item(2),
            supplied_slots: Some(vec![u16::MAX, 0, 7]),
        };
        let candidate = equipment_intent::apply_private(
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
            max_slot_links: 3,
            max_fact_links: 9,
            max_extra_bytes: 3,
            max_visits: candidate.visits(),
            max_projection_bytes: serde_json::to_vec(&candidate).unwrap().len(),
            ..Limits::default()
        };
        assert!(
            equipment_intent::apply_private(
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
        for label in [
            "slot link",
            "fact link",
            "extra byte",
            "visit",
            "projection byte",
        ] {
            let mut under = exact;
            match label {
                "slot link" => under.max_slot_links -= 1,
                "fact link" => under.max_fact_links -= 1,
                "extra byte" => under.max_extra_bytes -= 1,
                "visit" => under.max_visits -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                _ => unreachable!(),
            }
            assert!(
                matches!(equipment_intent::apply_private(&before,scripts,content,sources,&request,WorldLimits::default(),under),Err(Error::Capacity(found)) if found==label)
            );
        }
        let under = WorldLimits {
            max_item_links: 8,
            ..WorldLimits::default()
        };
        assert!(
            equipment_intent::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                under,
                Limits::default()
            )
            .is_err()
        );
    });
}
#[test]
fn connected_state_equipment_transfer_chain_cold_restores_and_exports_exact_stages() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let original = world(scripts).snapshot();
        let first_request = choice(&original, 1, 0x100);
        let first = reference_intent::apply_private(
            &original,
            scripts,
            content,
            sources,
            &first_request,
            WorldLimits::default(),
            Limits::default(),
        )
        .unwrap();
        let second_request = Choice {
            claim: claim(first.snapshot(), 1, 0x100),
            item: item(2),
            supplied_slots: Some(vec![65535, 0, 7]),
        };
        let second = equipment_intent::apply_private(
            first.snapshot(),
            scripts,
            content,
            sources,
            &second_request,
            WorldLimits::default(),
            Limits::default(),
        )
        .unwrap();
        let third_request = inventory_transfer::Choice {
            claim: claim(second.snapshot(), 1, 0x100),
            item: item(2),
            destination: reference(2),
        };
        let third = inventory_transfer::apply_private(
            second.snapshot(),
            scripts,
            content,
            sources,
            &third_request,
            WorldLimits::default(),
            Limits::default(),
        )
        .unwrap();
        for snapshot in [first.snapshot(), second.snapshot(), third.snapshot()] {
            assert_cold(scripts, snapshot);
        }
        conserve(&original, third.snapshot(), true, true, 3);
        let cold =
            World::restore(scripts, third.snapshot().clone(), WorldLimits::default()).unwrap();
        assert_eq!(cold.item(item(2)).unwrap().owner(), reference(2));
        assert_eq!(
            cold.item(item(2)).unwrap().facts().equipped_slots,
            Some(vec![65535, 0, 7])
        );
        assert!(
            inventory_transfer::apply_private(
                first.snapshot(),
                scripts,
                content,
                sources,
                &third_request,
                WorldLimits::default(),
                Limits::default()
            )
            .is_err()
        );
        if let Some(path) = std::env::var_os("FALLOUT_ACTOR_INTERACTIONS_EVIDENCE_DIR") {
            let path = std::path::PathBuf::from(path);
            for (name, candidate) in [
                ("chain-1", &first),
                ("chain-2", &second),
                ("chain-3", &third),
            ] {
                assert!(!path.join(format!("{name}-host.json")).exists());
                std::fs::write(
                    path.join(format!("{name}-host.json")),
                    serde_json::to_vec(candidate).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    path.join(format!("{name}-snapshot.json")),
                    candidate
                        .snapshot()
                        .encode(WorldLimits::default().max_snapshot_bytes)
                        .unwrap(),
                )
                .unwrap();
            }
            std::fs::write(
                path.join("chain-2-request.json"),
                serde_json::to_vec(&second_request).unwrap(),
            )
            .unwrap();
            std::fs::write(
                path.join("chain-3-request.json"),
                serde_json::to_vec(&third_request).unwrap(),
            )
            .unwrap();
        }
    });
}
