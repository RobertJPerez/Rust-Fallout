#[path = "actor_rules_context_batch_fixture.rs"]
mod fixture;
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::{
        context,
        context_batch::{self, Error, Limits},
    },
};
use fixture::*;
use std::fs;

#[test]
fn ordered_mixed_contexts_share_exact_sorted_bases_and_preserve_current_facts() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let before = world.snapshot();
        let ids = [
            reference(3),
            reference(1),
            reference(8),
            reference(2),
            reference(9),
        ];
        let batch = context_batch::observe_batch(
            &world,
            content,
            sources.placements,
            sources.actors,
            &ids,
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            batch
                .base_sources()
                .iter()
                .map(|a| a.key.local_id)
                .collect::<Vec<_>>(),
            [0x100, 0x101, 0x200]
        );
        assert_eq!(
            batch
                .references()
                .iter()
                .map(|r| r.base_source_index())
                .collect::<Vec<_>>(),
            [2, 0, 0, 1, 0]
        );
        for (id, row) in ids.into_iter().zip(batch.references()) {
            let single = context::observe(
                &world,
                content,
                sources.placements,
                sources.actors,
                id,
                Default::default(),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(row.reference()).unwrap(),
                serde_json::to_value(&single.reference).unwrap()
            );
            assert_eq!(
                serde_json::to_value(row.placement()).unwrap(),
                serde_json::to_value(single.placement).unwrap()
            );
            assert_eq!(
                serde_json::to_value(batch.base_sources()[row.base_source_index()]).unwrap(),
                serde_json::to_value(single.actor).unwrap()
            );
        }
        assert!(!batch.references()[1].reference().state().unwrap().enabled());
        assert!(batch.references()[0].reference().state().is_none());
        let reversed = ids.into_iter().rev().collect::<Vec<_>>();
        let other = context_batch::observe_batch(
            &world,
            content,
            sources.placements,
            sources.actors,
            &reversed,
            Default::default(),
        )
        .unwrap();
        let mut expected = serde_json::to_value(&batch).unwrap();
        expected["references"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(other).unwrap(), expected);
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn many_same_base_references_charge_one_cohort_and_one_base_projection() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let ids = (8..72).map(reference).collect::<Vec<_>>();
        let batch = context_batch::observe_batch(
            &world,
            content,
            sources.placements,
            sources.actors,
            &ids,
            Default::default(),
        )
        .unwrap();
        assert_eq!(batch.base_sources().len(), 1);
        let u = batch.usage();
        assert_eq!(u.source_admissions, 1);
        assert_eq!(u.source_receipt_visits, 2);
        assert_eq!(u.placement_fields, 64 * 4);
        assert_eq!(u.actor_fields, 5);
        assert_eq!(u.total_fields, 261);
        assert_eq!(u.visits, 2 + 64 + 261 + 7 * 64);
        let exact = Limits {
            max_references: 64,
            max_unique_bases: 1,
            max_total_fields: 261,
            max_visits: u.visits,
            max_current_bytes: u.current_view_bytes,
            max_projection_bytes: serde_json::to_vec(&batch).unwrap().len(),
            ..Default::default()
        };
        assert!(
            context_batch::observe_batch(
                &world,
                content,
                sources.placements,
                sources.actors,
                &ids,
                exact
            )
            .is_ok()
        );
        for label in [
            "reference",
            "base",
            "field",
            "visit",
            "current byte",
            "projection byte",
        ] {
            let mut under = exact;
            match label {
                "reference" => under.max_references -= 1,
                "base" => under.max_unique_bases -= 1,
                "field" => under.max_total_fields -= 1,
                "visit" => under.max_visits -= 1,
                "current byte" => under.max_current_bytes -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                _ => unreachable!(),
            }
            assert!(
                matches!(context_batch::observe_batch(&world,content,sources.placements,sources.actors,&ids,under),Err(Error::Capacity(found)) if found==label)
            );
        }
    });
}
#[test]
fn duplicates_and_invalid_late_rows_refuse_the_complete_group_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let before = world.snapshot();
        assert!(matches!(
            context_batch::observe_batch(
                &world,
                content,
                sources.placements,
                sources.actors,
                &[reference(1), reference(1)],
                Default::default()
            ),
            Err(Error::DuplicateReference)
        ));
        for id in [4, 5, 6, 7, 72, 999] {
            assert!(
                context_batch::observe_batch(
                    &world,
                    content,
                    sources.placements,
                    sources.actors,
                    &[reference(1), reference(id)],
                    Default::default()
                )
                .is_err()
            );
        }
        let empty = context_batch::observe_batch(
            &world,
            content,
            sources.placements,
            sources.actors,
            &[],
            Default::default(),
        )
        .unwrap();
        assert!(empty.references().is_empty() && empty.base_sources().is_empty());
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn borrowed_byte_admission_precedes_oversized_late_origin_source_join() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, false);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut snapshot = world(scripts).snapshot();
        // This valid but oversized origin is absent from source. Counting the
        // borrowed final row must precede source lookup and View allocation.
        let mut live = World::restore(scripts, snapshot.clone(), WorldLimits::default()).unwrap();
        let mut origin = form(0x500);
        origin.origin_plugin = "x".repeat(16384);
        let id = live.register_reference(Some(origin)).unwrap();
        snapshot = live.snapshot();
        assert!(matches!(
            context_batch::observe_batch(
                &live,
                content,
                sources.placements,
                sources.actors,
                &[reference(1), id],
                Limits {
                    max_current_bytes: 4096,
                    ..Default::default()
                }
            ),
            Err(Error::Capacity("current byte"))
        ));
        assert_eq!(live.snapshot(), snapshot);
    });
}
#[test]
fn source_override_and_changed_whole_cohort_cannot_lend_old_base_identity() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 2, 0, true);
    with_sources(temp.path(), |scripts, content, sources| {
        let world = world(scripts);
        let batch = context_batch::observe_batch(
            &world,
            content,
            sources.placements,
            sources.actors,
            &[reference(1), reference(8)],
            Default::default(),
        )
        .unwrap();
        assert_eq!(batch.base_sources()[0].source.plugin, "ActorPatch.esp");
        assert_eq!(batch.references()[0].base_source_index(), 1);
        assert_eq!(batch.references()[1].base_source_index(), 0);
        let other = tempfile::tempdir().unwrap();
        fixture(other.path(), 3, 0, true);
        with_sources(other.path(), |_, _, changed| {
            assert!(matches!(
                context_batch::observe_batch(
                    &world,
                    content,
                    changed.placements,
                    sources.actors,
                    &[reference(1)],
                    Default::default()
                ),
                Err(Error::Context(context::Error::ContextChanged))
            ))
        });
    });
}
#[test]
fn export_authored_batch_inputs_when_requested() {
    let Some(root) = std::env::var_os("FALLOUT_ACTOR_DECISION_INPUTS_EVIDENCE_DIR") else {
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
            for (name, ids) in [
                (
                    "mixed",
                    vec![
                        reference(3),
                        reference(1),
                        reference(8),
                        reference(2),
                        reference(9),
                    ],
                ),
                ("many", (8..72).map(reference).collect()),
            ] {
                let output = context_batch::observe_batch(
                    &cold,
                    content,
                    sources.placements,
                    sources.actors,
                    &ids,
                    Default::default(),
                )
                .unwrap();
                fs::write(
                    path.join(format!("{name}-selected.json")),
                    serde_json::to_vec(&ids).unwrap(),
                )
                .unwrap();
                fs::write(
                    path.join(format!("host-{name}.json")),
                    serde_json::to_vec(&output).unwrap(),
                )
                .unwrap();
            }
            assert_eq!(cold.snapshot(), snapshot);
        });
    }
}
