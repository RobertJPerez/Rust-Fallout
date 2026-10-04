use super::super::test_sources::{Fixture, POSE, key, members, record, reference, teleport, until};
use super::*;
use crate::{plugin, resource_jobs::tests::Pause};
use sha2::{Digest, Sha256};
fn host(f: &Fixture, l: DoorPrefetchLimits) -> DoorPrefetcher {
    DoorPrefetcher::new(f.root.path(), Some(f.cache.path()), l).unwrap()
}
fn requested(f: &Fixture, h: &mut DoorPrefetcher) -> Ticket {
    let mut store = f.store();
    let destination = f.destination(&mut store);
    h.request(&mut store, destination, &f.mounts, Default::default())
        .unwrap()
}
fn models(h: &mut DoorPrefetcher) {
    until(|| h.poll().unwrap().residency.stage == Stage::Decoded);
}
fn complete(f: &Fixture, h: &mut DoorPrefetcher, t: &Ticket) {
    models(h);
    h.prepare_textures(t, &f.mounts, Default::default())
        .unwrap();
    until(|| h.poll().unwrap().source_lease_available);
}
#[test]
fn exact_door_parent_pose_and_actual_model_texture_jobs_remain_ticket_scoped() {
    let f = Fixture::new();
    let mut h = host(&f, Default::default());
    let t = requested(&f, &mut h);
    assert_eq!(t.root(), &key(0x200));
    assert!(h.sources(&t).is_err());
    complete(&f, &mut h, &t);
    let lease = h.sources(&t).unwrap();
    let destination = lease.destination().unwrap().destination.as_ref().unwrap();
    assert_eq!(destination.cell, key(0x200));
    assert_eq!(destination.door, key(0x311));
    assert_eq!(destination.authored_transform_words, POSE);
    assert_eq!(destination.raw_flags, 0x81234567);
    assert_ne!(destination.authored_transform.position, [99.0, 88.0, 77.0]);
    assert_eq!(lease.models().unwrap().model(0).unwrap(), f.models[0]);
    assert_eq!(lease.texture(0).unwrap(), f.textures[0]);
    assert_eq!(lease.texture_receipt().unwrap().requests.len(), 1);
    assert_eq!(lease.ticket().generation(), t.generation());
    assert_eq!(
        format!("{:x}", Sha256::digest(lease.texture(0).unwrap())),
        format!("{:x}", Sha256::digest(&f.textures[0]))
    );
    let same = h.sources(&t).unwrap();
    assert!(Arc::ptr_eq(&same, &lease));
    drop(same);
    let before = h.snapshot();
    assert_eq!(before.residency.outstanding, 2);
    assert_eq!(before.retained_requests, 1);
    assert_eq!(
        before.residency.pinned_source_bytes,
        f.models[0].len() + f.textures[0].len()
    );
    assert!(!before.current_cell_changed);
    assert!(!before.authored_pose_applied);
    assert!(!before.runtime_ready);
    h.cancel().unwrap();
    assert!(lease.destination().is_err());
    assert!(lease.models().is_err());
    assert!(lease.texture(0).is_err());
    let retired = h.poll().unwrap();
    assert_eq!(retired.residency.outstanding, 2);
    assert_eq!(retired.retained_source_bytes, before.retained_source_bytes);
    assert_eq!(retired.retained_requests, 1);
    let next = requested(&f, &mut h);
    assert!(next.generation() > t.generation());
    models(&mut h);
    h.prepare_textures(&next, &f.mounts, Default::default())
        .unwrap();
    until(|| h.poll().unwrap().source_lease_available);
    assert_eq!(h.snapshot().retained_requests, 2);
    assert!(h.sources(&t).is_err());
    assert_eq!(h.sources(&next).unwrap().texture(0).unwrap(), f.textures[0]);
    drop(lease);
    assert_eq!(h.snapshot().retained_requests, 1);
    h.cancel().unwrap();
    until(|| h.poll().unwrap().residency.stage == Stage::Unrequested);
    assert_eq!(h.snapshot().retained_source_bytes, 0);
    assert_eq!(h.snapshot().residency.mapped_source_bytes, 0);
}
#[test]
fn stale_cohort_wrong_deleted_moved_and_cycle_sources_never_return_a_destination_lease() {
    let cases = vec![
        members(0x200, &reference(0x311, 0x450, plugin::DELETED, &[])),
        members(0x200, &reference(0x311, 0x400, 0, &[])),
        members(0x202, &reference(0x310, 0x450, 0, &teleport(0x311))), // source moved out of requested CELL.
        members(0x200, &reference(0x311, 0x450, 0, &teleport(0x310))), // cyclic pair.
        record(b"STAT", 0x311, 0, &[]),
    ];
    for patch in cases {
        let mut f = Fixture::new();
        f.patch(&patch);
        let mut store = f.store();
        let d = f.destination(&mut store);
        let mut h = host(&f, Default::default());
        assert!(
            h.request(&mut store, d, &f.mounts, Default::default())
                .is_err()
        );
        let snapshot = h.snapshot();
        assert!(!snapshot.source_lease_available);
        assert_eq!(snapshot.retained_requests, 0);
        assert_eq!(snapshot.residency.retained_plans, 0);
    }
    let mut f = Fixture::new();
    let mut old = f.store();
    let d = f.destination(&mut old);
    drop(old);
    f.patch(&record(b"STAT", 0x499, 0, &[]));
    let mut changed = f.store();
    let mut h = host(&f, Default::default());
    assert!(
        h.request(&mut changed, d, &f.mounts, Default::default())
            .is_err()
    );
    assert_eq!(h.snapshot().retained_requests, 0);
}
#[test]
fn retention_refusal_preserves_live_prefetch_and_late_texture_budget_failure_has_no_lease() {
    let f = Fixture::new();
    let mut h = host(
        &f,
        DoorPrefetchLimits {
            retained_requests: 1,
            ..Default::default()
        },
    );
    let t = requested(&f, &mut h);
    complete(&f, &mut h, &t);
    let held = h.sources(&t).unwrap();
    let mut store = f.store();
    let d = f.destination(&mut store);
    assert!(matches!(
        h.request(&mut store, d, &f.mounts, Default::default()),
        Err(JobError::QueueFull)
    ));
    assert_eq!(held.texture(0).unwrap(), f.textures[0]);
    t.check().unwrap();
    h.cancel().unwrap();
    assert_eq!(h.snapshot().retained_requests, 1);
    let mut store = f.store();
    let d = f.destination(&mut store);
    assert!(
        h.request(&mut store, d, &f.mounts, Default::default())
            .is_err()
    );
    drop(held);
    let t2 = requested(&f, &mut h);
    assert!(t2.generation() > t.generation());
    h.cancel().unwrap();
    let mut low = host(
        &f,
        DoorPrefetchLimits {
            residency: Limits {
                source_bytes: f.models[0].len() + f.textures[0].len() - 1,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let t = requested(&f, &mut low);
    models(&mut low);
    assert!(
        low.prepare_textures(&t, &f.mounts, Default::default())
            .is_err()
    );
    assert!(low.sources(&t).is_err());
    assert!(!low.snapshot().source_lease_available);
    assert!(low.snapshot().source_error.is_some());
    low.cancel().unwrap();
    until(|| low.poll().unwrap().residency.stage == Stage::Unrequested);
    let mut exact = host(
        &f,
        DoorPrefetchLimits {
            residency: Limits {
                source_bytes: f.models[0].len() + f.textures[0].len(),
                models: 1,
                resources: 2,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let t = requested(&f, &mut exact);
    complete(&f, &mut exact, &t);
    assert!(exact.sources(&t).is_ok());
    let mut queue = host(
        &f,
        DoorPrefetchLimits {
            residency: Limits {
                resources: 1,
                models: 1,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let t = requested(&f, &mut queue);
    models(&mut queue);
    assert!(
        queue
            .prepare_textures(&t, &f.mounts, Default::default())
            .is_err()
    );
    assert!(!queue.snapshot().source_lease_available);
}
#[test]
fn graph_retention_exact_and_one_under_and_foreign_owner_tickets_are_checked() {
    let f = Fixture::new();
    let mut a = host(&f, Default::default());
    let ta = requested(&f, &mut a);
    let bytes = a.snapshot().retained_source_bytes;
    let mut b = host(
        &f,
        DoorPrefetchLimits {
            retained_source_bytes: bytes,
            ..Default::default()
        },
    );
    let tb = requested(&f, &mut b);
    assert_eq!(ta.generation(), tb.generation());
    assert_eq!(ta.identity(), tb.identity());
    complete(&f, &mut a, &ta);
    assert!(a.sources(&tb).is_err());
    assert!(
        a.prepare_textures(&tb, &f.mounts, Default::default())
            .is_err()
    );
    assert!(a.sources(&ta).is_ok());
    let mut low = host(
        &f,
        DoorPrefetchLimits {
            retained_source_bytes: bytes - 1,
            ..Default::default()
        },
    );
    let mut store = f.store();
    let d = f.destination(&mut store);
    assert!(
        low.request(&mut store, d, &f.mounts, Default::default())
            .is_err()
    );
    assert_eq!(low.snapshot().retained_requests, 0);
    assert_eq!(low.snapshot().residency.retained_plans, 0);
}
struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
#[test]
fn cancelled_model_and_texture_outputs_before_and_after_extraction_cannot_publish() {
    for textures in [false, true] {
        for after in [false, true] {
            let f = Fixture::new();
            let mut h = host(&f, Default::default());
            let t = requested(&f, &mut h);
            if textures {
                models(&mut h);
            }
            let pause = Pause::new(after);
            h.owner.pause = Some(pause.clone());
            let _release = Release(pause.clone());
            if textures {
                h.prepare_textures(&t, &f.mounts, Default::default())
                    .unwrap();
            }
            let cache_before = std::fs::read_dir(f.cache.path()).unwrap().count();
            h.poll().unwrap();
            pause.reached();
            h.cancel().unwrap();
            assert!(t.check().is_err());
            assert!(!h.snapshot().source_lease_available);
            pause.release();
            pause.completed();
            until(|| h.poll().unwrap().residency.stage == Stage::Unrequested);
            assert!(h.sources(&t).is_err());
            assert_eq!(h.snapshot().residency.pinned_source_bytes, 0);
            assert_eq!(h.snapshot().retained_requests, 0);
            assert_eq!(
                std::fs::read_dir(f.cache.path()).unwrap().count(),
                cache_before
            );
            h.owner.pause = None;
            let next = requested(&f, &mut h);
            complete(&f, &mut h, &next);
            assert_eq!(h.sources(&next).unwrap().texture(0).unwrap(), f.textures[0]);
        }
    }
}
