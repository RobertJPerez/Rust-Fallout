#[path = "actor_rules_context_batch_fixture.rs"]
mod fixture;
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::faction_pair::{self, Error, Limits},
};
use fixture::*;
use std::fs;
fn tuples(pair: &faction_pair::DirectedPairInputs<'_>) -> Vec<(usize, usize, usize, i32, u32)> {
    pair.relations
        .iter()
        .map(|r| {
            (
                r.source_occurrence_index,
                r.relationship_field_index,
                r.target_occurrence_index,
                r.modifier,
                r.group_combat_reaction,
            )
        })
        .collect()
}
#[test]
fn directed_physical_occurrences_preserve_multiplicity_signed_modifiers_and_raw_reactions() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let before = world.snapshot();
        let pair = faction_pair::observe_pair(
            &world,
            content,
            sources.placements,
            sources.actors,
            sources.associations,
            sources.factions,
            reference(1),
            reference(2),
            Default::default(),
        )
        .unwrap();
        assert_eq!(pair.from.context.actor.key, &form(0x100));
        assert_eq!(pair.to.context.actor.key, &form(0x101));
        assert_eq!(
            tuples(&pair),
            [
                (0, 1, 0, -13, u32::MAX),
                (0, 1, 1, -13, u32::MAX),
                (0, 2, 0, 17, 0x80000000),
                (0, 2, 1, 17, 0x80000000),
                (1, 1, 0, 123, 5),
                (1, 1, 1, 123, 5),
                (2, 1, 0, -13, u32::MAX),
                (2, 1, 1, -13, u32::MAX),
                (2, 2, 0, 17, 0x80000000),
                (2, 2, 1, 17, 0x80000000)
            ]
        );
        assert_eq!(
            pair.from
                .memberships
                .iter()
                .map(|m| m.association.faction_rank)
                .collect::<Vec<_>>(),
            [Some(-2), Some(7), Some(-1)]
        );
        assert_eq!(
            pair.to
                .memberships
                .iter()
                .map(|m| m.association.faction_rank)
                .collect::<Vec<_>>(),
            [Some(3), Some(-7)]
        );
        assert!(
            pair.relations
                .iter()
                .all(|r| r.source_field_index == r.source_occurrence_index + 2
                    && r.target_field_index == r.target_occurrence_index + 2)
        );
        let reverse = faction_pair::observe_pair(
            &world,
            content,
            sources.placements,
            sources.actors,
            sources.associations,
            sources.factions,
            reference(2),
            reference(1),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            tuples(&reverse),
            [
                (0, 1, 0, -71, 0x7fff0001),
                (0, 1, 2, -71, 0x7fff0001),
                (0, 2, 1, 9, 4),
                (1, 1, 0, -71, 0x7fff0001),
                (1, 1, 2, -71, 0x7fff0001),
                (1, 2, 1, 9, 4)
            ]
        );
        assert_eq!(reverse.unavailable_relations.len(), 2);
        assert!(
            reverse
                .unavailable_relations
                .iter()
                .all(|r| r.relationship_field_index == 3)
        );
        assert!(!pair.effective_membership_supported && !pair.reaction_evaluation_supported);
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn template_and_unresolved_membership_remain_unavailable_and_same_endpoint_has_no_defaults() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let pair = faction_pair::observe_pair(
            &world,
            content,
            sources.placements,
            sources.actors,
            sources.associations,
            sources.factions,
            reference(3),
            reference(1),
            Default::default(),
        )
        .unwrap();
        assert!(
            pair.from
                .issues
                .iter()
                .any(|i| i.code == "faction_template_selection_unsupported")
        );
        assert_eq!(
            pair.from.memberships[1].unavailable,
            "authored_faction_unavailable"
        );
        assert!(pair.from.memberships[1].faction.is_none());
        let same = faction_pair::observe_pair(
            &world,
            content,
            sources.placements,
            sources.actors,
            sources.associations,
            sources.factions,
            reference(1),
            reference(1),
            Default::default(),
        )
        .unwrap();
        assert!(same.relations.is_empty());
    });
}
#[test]
fn fresh_current_origin_and_cold_restore_do_not_borrow_a_different_base_or_old_handle() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, true);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut live = world(scripts);
        let old = live.reference_view(reference(1)).unwrap();
        let snapshot = live.snapshot();
        live.replace_from_snapshot(snapshot.clone()).unwrap();
        assert!(
            live.stage_reference_state(&old, old.state().unwrap().clone())
                .is_err()
        );
        let pair = faction_pair::observe_pair(
            &live,
            content,
            sources.placements,
            sources.actors,
            sources.associations,
            sources.factions,
            reference(1),
            reference(8),
            Default::default(),
        )
        .unwrap();
        assert_eq!(pair.from.context.actor.key, &form(0x101));
        assert_eq!(pair.to.context.actor.key, &form(0x100));
        assert_eq!(pair.from.context.placement.source.plugin, "ActorPatch.esp");
        let cold = World::restore(scripts, snapshot.clone(), WorldLimits::default()).unwrap();
        let reb = faction_pair::observe_pair(
            &cold,
            content,
            sources.placements,
            sources.actors,
            sources.associations,
            sources.factions,
            reference(1),
            reference(8),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(pair).unwrap(),
            serde_json::to_value(reb).unwrap()
        );
        assert_eq!(cold.snapshot(), snapshot);
    });
}
#[test]
fn exact_aggregate_pair_output_and_source_budgets_refuse_one_less_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let before = world.snapshot();
        let pair = faction_pair::observe_pair(
            &world,
            content,
            sources.placements,
            sources.actors,
            sources.associations,
            sources.factions,
            reference(2),
            reference(1),
            Default::default(),
        )
        .unwrap();
        let u = pair.usage;
        let exact = Limits {
            max_occurrences: u.occurrences,
            max_faction_fields: u.faction_fields,
            max_relation_pairs: u.relation_pairs,
            max_unavailable_relations: u.unavailable_relations,
            max_visits: u.visits,
            max_current_bytes: u.current_view_bytes,
            max_projection_bytes: serde_json::to_vec(&pair).unwrap().len(),
            ..Default::default()
        };
        assert!(
            faction_pair::observe_pair(
                &world,
                content,
                sources.placements,
                sources.actors,
                sources.associations,
                sources.factions,
                reference(2),
                reference(1),
                exact
            )
            .is_ok()
        );
        for label in [
            "occurrence",
            "faction field",
            "relation pair",
            "unavailable relation",
            "visit",
            "current byte",
            "projection byte",
        ] {
            let mut under = exact;
            match label {
                "occurrence" => under.max_occurrences -= 1,
                "faction field" => under.max_faction_fields -= 1,
                "relation pair" => under.max_relation_pairs -= 1,
                "unavailable relation" => under.max_unavailable_relations -= 1,
                "visit" => under.max_visits -= 1,
                "current byte" => under.max_current_bytes -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                _ => unreachable!(),
            }
            assert!(
                matches!(faction_pair::observe_pair(&world,content,sources.placements,sources.actors,sources.associations,sources.factions,reference(2),reference(1),under),Err(Error::Capacity(found)) if found==label)
            );
        }
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn large_repeated_target_bucket_and_late_invalid_endpoint_refuse_whole_request() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 1024, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let before = world.snapshot();
        assert!(matches!(
            faction_pair::observe_pair(
                &world,
                content,
                sources.placements,
                sources.actors,
                sources.associations,
                sources.factions,
                reference(1),
                reference(2),
                Limits {
                    max_relation_pairs: 31,
                    ..Default::default()
                }
            ),
            Err(Error::Capacity("relation pair"))
        ));
        for id in [4, 5, 6, 7, 72, 999] {
            assert!(
                faction_pair::observe_pair(
                    &world,
                    content,
                    sources.placements,
                    sources.actors,
                    sources.associations,
                    sources.factions,
                    reference(1),
                    reference(id),
                    Default::default()
                )
                .is_err()
            );
        }
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn changed_payload_with_equal_headers_cannot_supply_other_endpoint_faction_requests() {
    let first = tempfile::tempdir().unwrap();
    fixture(first.path(), 2, 0, false);
    let other = tempfile::tempdir().unwrap();
    fixture(other.path(), 3, 0, false);
    with_sources(first.path(), |scripts, content, sources| {
        let world = world(scripts);
        let before = world.snapshot();
        with_sources(other.path(), |_, _, changed| {
            assert!(
                faction_pair::observe_pair(
                    &world,
                    content,
                    sources.placements,
                    sources.actors,
                    changed.associations,
                    changed.factions,
                    reference(1),
                    reference(2),
                    Default::default()
                )
                .is_err()
            )
        });
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn export_pair_host_observations_when_requested() {
    let Some(root) = std::env::var_os("FALLOUT_ACTOR_DECISION_INPUTS_PAIR_EVIDENCE_DIR") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    fs::create_dir_all(&root).unwrap();
    for (name, overrides) in [("base", false), ("override", true)] {
        let path = root.join(name);
        assert!(!path.exists());
        fixture(&path, 2, 0, overrides);
        with_sources(&path, |scripts, content, sources| {
            let snapshot = world(scripts).snapshot();
            fs::write(
                path.join("snapshot.json"),
                snapshot
                    .encode(WorldLimits::default().max_snapshot_bytes)
                    .unwrap(),
            )
            .unwrap();
            let cold = World::restore(scripts, snapshot.clone(), WorldLimits::default()).unwrap();
            for (name, from, to) in [
                ("forward", 1, 2),
                ("reverse", 2, 1),
                ("template", 3, 1),
                ("same", 1, 1),
            ] {
                let pair = faction_pair::observe_pair(
                    &cold,
                    content,
                    sources.placements,
                    sources.actors,
                    sources.associations,
                    sources.factions,
                    reference(from),
                    reference(to),
                    Default::default(),
                )
                .unwrap();
                let choice = serde_json::json!({"from_reference":from,"to_reference":to,"expected_from_actor":pair.from.context.actor.key,"expected_to_actor":pair.to.context.actor.key});
                fs::write(
                    path.join(format!("{name}-choice.json")),
                    serde_json::to_vec(&choice).unwrap(),
                )
                .unwrap();
                fs::write(
                    path.join(format!("host-{name}.json")),
                    serde_json::to_vec(&pair).unwrap(),
                )
                .unwrap();
            }
            assert_eq!(cold.snapshot(), snapshot);
        });
    }
}
