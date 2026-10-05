use super::super::test_sources::{Fixture, key, until};
use super::*;
use crate::resource_jobs::tests::Pause;
use std::sync::Arc;

fn set_limits(fixture: &Fixture, slots: usize) -> SetLimits {
    SetLimits {
        slots,
        workers: slots,
        models: 8,
        resources: 8,
        source_bytes: 8 * fixture.models.iter().map(Vec::len).max().unwrap(),
        ..Default::default()
    }
}

fn manager(fixture: &Fixture, plans: &[CellModelPlan], slots: usize) -> AdjacentRegionResidency {
    AdjacentRegionResidency::new(
        fixture.root.path(),
        Some(fixture.cache.path()),
        &plans[0].receipt().source_cohort_sha256,
        set_limits(fixture, slots),
        RegionPolicy::new(1, 1).unwrap(),
    )
    .unwrap()
}

fn request<'a>(
    worldspace: &'a FormKey,
    grid: [i32; 2],
    plan: &'a CellModelPlan,
    priority: u8,
) -> RegionRequest<'a> {
    RegionRequest::new(worldspace, grid, plan.root(), plan, priority).unwrap()
}

struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[test]
fn admission_priority_orders_work_and_wait_age_keeps_polling_fair() {
    let fixture = Fixture::new();
    let plans = fixture.plans();
    let worldspace = key(0x1000);
    let mut regions = manager(&fixture, &plans, 2);
    let requests = [
        request(&worldspace, [0, 0], &plans[0], 1),
        request(&worldspace, [1, 0], &plans[1], 9),
        request(&worldspace, [0, 1], &plans[2], 5),
    ];

    let update = regions.reconcile(&worldspace, [0, 0], &requests).unwrap();
    let tickets = regions.active_tickets();
    assert_eq!(update.admitted, 2);
    assert_eq!(update.deferred, 1);
    assert_eq!(tickets[0].identity().key.grid, [1, 0]);
    assert_eq!(tickets[0].identity().key.cell, *plans[1].root());
    assert_eq!(tickets[1].identity().key.grid, [0, 1]);
    assert_eq!(tickets[1].identity().key.cell, *plans[2].root());

    let first = regions.poll(1).unwrap();
    let first_slot = first.last_polled_slots[0];
    assert_eq!(first.slots[first_slot].root.as_ref(), Some(plans[1].root()));
    let second = regions.poll(1).unwrap();
    let second_slot = second.last_polled_slots[0];
    assert_eq!(
        second.slots[second_slot].root.as_ref(),
        Some(plans[2].root())
    );
}

#[test]
fn reverse_crossing_keeps_overlapping_roots_inside_the_retention_band() {
    let fixture = Fixture::new();
    let plans = fixture.plans();
    let worldspace = key(0x1000);
    let mut regions = manager(&fixture, &plans, 3);
    let first_window = [
        request(&worldspace, [0, 0], &plans[0], 8),
        request(&worldspace, [1, 0], &plans[1], 4),
    ];
    let initial = regions
        .reconcile(&worldspace, [0, 0], &first_window)
        .unwrap();
    assert_eq!(initial.admitted, 2);
    let first_a = regions
        .active_tickets()
        .into_iter()
        .find(|ticket| ticket.identity().key.cell == *plans[0].root())
        .unwrap();

    let next_window = [
        request(&worldspace, [1, 0], &plans[1], 8),
        request(&worldspace, [2, 0], &plans[2], 7),
    ];
    let moved = regions
        .reconcile(&worldspace, [1, 0], &next_window)
        .unwrap();
    assert_eq!(moved.retained, 2);
    assert_eq!(moved.admitted, 1);
    assert_eq!(moved.active_regions, 3);
    let retained_a = regions
        .active_tickets()
        .into_iter()
        .find(|ticket| ticket.identity().key.cell == *plans[0].root())
        .unwrap();
    assert_eq!(retained_a.identity(), first_a.identity());

    let reversed = regions
        .reconcile(&worldspace, [0, 0], &first_window)
        .unwrap();
    assert_eq!(reversed.retired, 1);
    assert_eq!(reversed.active_regions, 2);
    assert!(regions.is_current(&first_a));
    assert_eq!(
        regions
            .active_tickets()
            .iter()
            .filter(|ticket| ticket.identity().key.cell == *plans[0].root())
            .count(),
        1
    );
}

#[test]
fn world_instance_replacement_rejects_a_late_reused_cell_and_block_completion() {
    let fixture = Fixture::new();
    let plans = fixture.plans();
    let worldspace = key(0x1000);
    let reused_grid = [7, -4];
    let mut regions = manager(&fixture, &plans, 1);
    let old_request = [request(&worldspace, reused_grid, &plans[0], 9)];
    let old_update = regions
        .reconcile(&worldspace, reused_grid, &old_request)
        .unwrap();
    assert_eq!(old_update.world_generation, 1);
    let old_ticket = regions.active_tickets().pop().unwrap();

    let pause = Pause::new(true);
    regions
        .owner
        .pause_test_work(&old_ticket.source, pause.clone())
        .unwrap();
    let _release = Release(pause.clone());
    regions.poll(1).unwrap();
    pause.reached();

    assert_eq!(regions.replace_worldspace(&worldspace).unwrap(), 1);
    assert_eq!(regions.world_generation(), 2);
    let new_request = [request(&worldspace, reused_grid, &plans[0], 9)];
    let waiting = regions
        .reconcile(&worldspace, reused_grid, &new_request)
        .unwrap();
    assert_eq!(waiting.deferred, 1);
    assert_eq!(waiting.active_regions, 0);
    assert!(old_ticket.check().is_err());
    let mut published = false;
    assert!(
        regions
            .publish_render(&old_ticket, || {
                published = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!published);

    pause.release();
    pause.completed();
    until(|| regions.poll(1).unwrap().usage.retired_slots == 0);
    assert!(regions.sources(&old_ticket).is_err());

    let admitted = regions
        .reconcile(&worldspace, reused_grid, &new_request)
        .unwrap();
    assert_eq!(admitted.admitted, 1);
    let new_ticket = regions.active_tickets().pop().unwrap();
    assert_eq!(old_ticket.identity().key, new_ticket.identity().key);
    assert_ne!(
        old_ticket.identity().world_generation,
        new_ticket.identity().world_generation
    );
    assert_ne!(
        old_ticket.identity().region_generation,
        new_ticket.identity().region_generation
    );
    assert!(regions.is_current(&new_ticket));
}

#[test]
fn foreign_manager_ticket_with_identical_identity_is_rejected() {
    let fixture = Fixture::new();
    let plans = fixture.plans();
    let worldspace = key(0x1000);
    let requests = [request(&worldspace, [0, 0], &plans[0], 9)];
    let mut left = manager(&fixture, &plans, 1);
    let mut right = manager(&fixture, &plans, 1);
    left.reconcile(&worldspace, [0, 0], &requests).unwrap();
    right.reconcile(&worldspace, [0, 0], &requests).unwrap();
    let foreign = right.active_tickets().pop().unwrap();
    let own = left.active_tickets().pop().unwrap();

    assert_eq!(foreign.identity(), own.identity());
    assert!(foreign.check().is_ok());
    assert!(!left.is_current(&foreign));
    assert!(left.sources(&foreign).is_err());
    let mut published = false;
    assert!(
        left.publish_render(&foreign, || {
            published = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!published);
}

#[test]
fn retired_owner_drains_while_active_region_is_preferred() {
    let fixture = Fixture::new();
    let plans = fixture.plans();
    let worldspace = key(0x1000);
    let mut regions = manager(&fixture, &plans, 2);
    let requests = [
        request(&worldspace, [0, 0], &plans[0], 9),
        request(&worldspace, [1, 0], &plans[1], 1),
    ];
    regions.reconcile(&worldspace, [0, 0], &requests).unwrap();
    let old = regions
        .active_tickets()
        .into_iter()
        .find(|ticket| ticket.identity().key.cell == *plans[0].root())
        .unwrap();
    let pause = Pause::new(true);
    regions
        .owner
        .pause_test_work(&old.source, pause.clone())
        .unwrap();
    let _release = Release(pause.clone());
    regions.poll(1).unwrap();
    pause.reached();

    let keep = [request(&worldspace, [1, 0], &plans[1], 1)];
    let crossed = regions.reconcile(&worldspace, [2, 0], &keep).unwrap();
    assert_eq!(crossed.retired, 1);
    assert_eq!(crossed.active_regions, 1);
    pause.release();
    pause.completed();
    for _ in 0..32 {
        regions.poll(1).unwrap();
    }
    assert_eq!(regions.snapshot().usage.retired_slots, 0);
}

#[test]
fn failed_source_ticket_can_be_removed_and_drained() {
    let fixture = Fixture::with_corrupt_model_archive();
    let plans = fixture.plans();
    let worldspace = key(0x1000);
    let mut regions = manager(&fixture, &plans, 1);
    let requests = [request(&worldspace, [0, 0], &plans[0], 9)];
    regions.reconcile(&worldspace, [0, 0], &requests).unwrap();
    let ticket = regions.active_tickets().pop().unwrap();

    until(|| {
        regions.poll(1).unwrap().slots.iter().any(|slot| {
            slot.source
                .as_ref()
                .is_some_and(|source| source.stage == Stage::Failed)
        })
    });
    assert!(ticket.check().is_err());
    regions.remove(&ticket).unwrap();
    assert!(regions.active_tickets().is_empty());
    assert_eq!(regions.snapshot().usage.active_slots, 0);
    assert_eq!(regions.snapshot().usage.retired_slots, 1);
    until(|| regions.poll(1).unwrap().usage.retired_slots == 0);
    assert_eq!(regions.snapshot().usage.outstanding, 0);
}
