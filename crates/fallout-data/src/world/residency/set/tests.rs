use super::super::test_sources::{Fixture, field, key, record, until};
use super::*;
use crate::resource_jobs::tests::Pause;
use sha2::{Digest, Sha256};
use std::fs;
fn limits(f: &Fixture) -> SetLimits {
    SetLimits {
        models: 8,
        resources: 8,
        source_bytes: 8 * f.models.iter().map(Vec::len).max().unwrap(),
        ..Default::default()
    }
}
fn manager(f: &Fixture, p: &[CellModelPlan], l: SetLimits) -> CellResidencySet {
    CellResidencySet::new(
        f.root.path(),
        Some(f.cache.path()),
        &p[0].receipt().source_cohort_sha256,
        l,
    )
    .unwrap()
}
fn admissions(p: &[CellModelPlan]) -> Vec<Admission<'_>> {
    p.iter()
        .map(|plan| Admission {
            root: plan.root(),
            plan,
        })
        .collect()
}
fn complete(s: &mut CellResidencySet, t: &[Ticket]) {
    until(|| {
        s.poll(1).unwrap();
        t.iter().all(|t| s.sources(t).is_ok())
    });
}
#[test]
fn three_cells_consume_distinct_bytes_remove_one_and_drain_only_its_pins() {
    let f = Fixture::new();
    let p = f.plans();
    let l = limits(&f);
    let mut s = manager(&f, &p, l);
    let t = s.admit(&admissions(&p)).unwrap();
    complete(&mut s, &t);
    let leases: Vec<_> = t.iter().map(|t| s.sources(t).unwrap()).collect();
    for (i, lease) in leases.iter().enumerate() {
        assert_eq!(lease.plan().unwrap().root(), &key(0x200 + i as u32));
        let expected = &f.models[i];
        let found = (0..lease.plan().unwrap().receipt().requests.len())
            .any(|n| lease.model(n).unwrap() == expected);
        assert!(found);
        assert_eq!(lease.ticket().identity(), p[i].identity());
    }
    let hashes: Vec<_> = f
        .models
        .iter()
        .map(|b| format!("{:x}", Sha256::digest(b)))
        .collect();
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    let before = s.snapshot();
    assert_eq!(before.usage.active_slots, 3);
    assert_eq!(before.usage.outstanding, 4);
    assert_eq!(
        before.usage.pinned_source_bytes,
        p.iter()
            .map(|p| p.receipt().usage.model_decoded_bytes)
            .sum::<usize>()
    );
    assert!(before.usage.workers <= l.workers);
    assert!(!before.simulation_ready);
    assert!(!before.runtime_ready);
    s.remove(&t[1]).unwrap();
    assert!(leases[1].model(0).is_err());
    let held = s.poll(4).unwrap();
    assert_eq!(held.usage.retired_slots, 1);
    assert_eq!(held.usage.outstanding, before.usage.outstanding);
    assert_eq!(held.usage.retained_plans, 3);
    assert_eq!(
        held.usage.mapped_source_bytes,
        before.usage.mapped_source_bytes
    );
    for i in [0, 2] {
        assert!(s.sources(&t[i]).is_ok());
        assert!(leases[i].model(0).is_ok());
    }
    let [first, removed, last]: [Arc<ResidentSources>; 3] = leases.try_into().ok().unwrap();
    drop(removed);
    until(|| s.poll(1).unwrap().usage.retired_slots == 0);
    let drained = s.snapshot();
    assert_eq!(drained.usage.active_slots, 2);
    assert_eq!(drained.usage.retained_plans, 2);
    assert_eq!(drained.usage.outstanding, 2);
    assert_eq!(drained.usage.workers, 2);
    assert!(first.model(0).is_ok());
    assert!(last.model(0).is_ok());
}
#[test]
fn last_preflight_failure_never_admits_a_prefix_or_revokes_an_existing_cell() {
    let f = Fixture::new();
    let p = f.plans();
    let mut s = manager(&f, &p, limits(&f));
    let t = s.admit(&admissions(&p[..1])).unwrap();
    complete(&mut s, &t);
    let held = s.sources(&t[0]).unwrap();
    let requests = [
        Admission {
            root: p[2].root(),
            plan: &p[2],
        },
        Admission {
            root: p[1].root(),
            plan: &p[2],
        },
    ];
    assert!(s.admit(&requests).is_err());
    assert_eq!(s.snapshot().usage.active_slots, 1);
    assert!(
        s.admit(&[Admission {
            root: p[0].root(),
            plan: &p[0]
        }])
        .is_err()
    );
    assert!(
        s.admit(&[
            Admission {
                root: p[1].root(),
                plan: &p[1]
            },
            Admission {
                root: p[1].root(),
                plan: &p[1]
            }
        ])
        .is_err()
    );
    let other = Fixture::new(); // same layouts, deliberately different source cohort.
    let mut other = other;
    other.patch(&record(b"STAT", 0x499, 0, &[]));
    let q = other.plans();
    assert!(
        s.admit(&[Admission {
            root: q[2].root(),
            plan: &q[2]
        }])
        .is_err()
    );
    assert_eq!(s.snapshot().usage.active_slots, 1);
    assert!(held.model(0).is_ok());
    t[0].check().unwrap();
    let mut small = manager(
        &f,
        &p,
        SetLimits {
            models: 4,
            resources: 4,
            ..limits(&f)
        },
    );
    assert!(matches!(
        small.admit(&admissions(&p[..2])),
        Err(JobError::QueueFull)
    ));
    assert_eq!(small.snapshot().usage.active_slots, 0);
    assert_eq!(small.snapshot().usage.workers, 0);
}
#[test]
fn exact_static_allowances_and_one_under_refuse_before_any_host_publication() {
    let f = Fixture::new();
    let p = f.plans();
    let max_bytes = p
        .iter()
        .map(|p| p.receipt().usage.model_decoded_bytes)
        .max()
        .unwrap();
    let max_meta = p
        .iter()
        .map(|p| p.receipt().usage.metadata_bytes)
        .max()
        .unwrap();
    let mapped = p
        .iter()
        .map(|p| {
            p.receipt()
                .archives
                .iter()
                .map(|a| a.source_bytes)
                .sum::<u64>()
        })
        .max()
        .unwrap();
    let mut l = SetLimits {
        slots: 3,
        workers: 3,
        models: 6,
        resources: 6,
        source_bytes: 3 * max_bytes,
        retained_plans: 3,
        plan_metadata_bytes: 3 * max_meta,
        mapped_source_bytes: 3 * mapped,
        ..Default::default()
    };
    let probe = manager(&f, &p, l);
    let base = probe.snapshot().usage.metadata_bytes;
    l.metadata_bytes = base
        + p.iter()
            .map(|p| 4096 + 16 * p.root().origin_plugin.len())
            .sum::<usize>();
    let mut exact = manager(&f, &p, l);
    let t = exact.admit(&admissions(&p)).unwrap();
    complete(&mut exact, &t);
    assert_eq!(exact.snapshot().usage.metadata_bytes, l.metadata_bytes);
    assert_eq!(exact.snapshot().usage.workers, l.workers);
    for variant in 0..4 {
        let mut lower = l;
        // Lower every partition by one: the largest request is last.
        match variant {
            0 => lower.source_bytes -= 3,
            1 => lower.plan_metadata_bytes -= 3,
            2 => lower.mapped_source_bytes -= 3,
            _ => lower.metadata_bytes -= 1,
        }
        let mut under = manager(&f, &p, lower);
        assert!(under.admit(&admissions(&p)).is_err(), "variant {variant}");
        assert_eq!(under.snapshot().usage.active_slots, 0);
        assert_eq!(under.snapshot().usage.workers, 0);
    }
    assert!(
        CellResidencySet::new(
            f.root.path(),
            None,
            &p[0].receipt().source_cohort_sha256,
            SetLimits {
                workers: 2,
                slots: 3,
                ..l
            }
        )
        .is_err()
    );
    assert!(
        CellResidencySet::new(
            f.root.path(),
            None,
            &p[0].receipt().source_cohort_sha256,
            SetLimits {
                retained_plans: 2,
                ..l
            }
        )
        .is_err()
    );
    assert!(exact.poll(0).is_err());
    assert!(exact.poll(4).is_err());
}
#[test]
fn replacement_is_atomic_and_foreign_equal_numbered_epochs_cannot_remove_cells() {
    let f = Fixture::new();
    let p = f.plans();
    let mut a = manager(&f, &p, limits(&f));
    let mut b = manager(&f, &p, limits(&f));
    let ta = a.admit(&admissions(&p[..1])).unwrap().remove(0);
    let tb = b.admit(&admissions(&p[..1])).unwrap().remove(0);
    complete(&mut a, std::slice::from_ref(&ta));
    complete(&mut b, std::slice::from_ref(&tb));
    assert_eq!(ta.generation(), tb.generation());
    assert_eq!(ta.identity(), tb.identity());
    assert!(a.remove(&tb).is_err());
    assert!(a.sources(&tb).is_err());
    ta.check().unwrap();
    let held = a.sources(&ta).unwrap();
    assert!(
        a.replace(
            &ta,
            Admission {
                root: p[1].root(),
                plan: &p[2]
            }
        )
        .is_err()
    );
    assert!(held.model(0).is_ok());
    let replacement = a
        .replace(
            &ta,
            Admission {
                root: p[0].root(),
                plan: &p[0],
            },
        )
        .unwrap();
    assert!(ta.check().is_err());
    assert!(held.model(0).is_err());
    tb.check().unwrap();
    assert_eq!(a.snapshot().usage.retired_slots, 1);
    complete(&mut a, std::slice::from_ref(&replacement));
    assert_eq!(a.snapshot().usage.retained_plans, 2);
    assert_eq!(ta.generation(), replacement.generation()); // distinct private host incarnations.
    assert!(a.remove(&ta).is_err());
    replacement.check().unwrap();
    drop(held);
    until(|| a.poll(4).unwrap().usage.retired_slots == 0);
}
#[test]
fn zero_model_lease_still_charges_its_retired_source_plan() {
    let f = Fixture::new();
    fs::write(
        f.root.path().join("Data/Base.esm"),
        [
            record(
                b"TES4",
                0,
                0,
                &field(
                    b"HEDR",
                    &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                ),
            ),
            record(b"CELL", 0x200, 0, &field(b"DATA", &[1])),
        ]
        .concat(),
    )
    .unwrap();
    let p =
        CellModelPlan::load(&mut f.store(), &key(0x200), &f.mounts, Default::default()).unwrap();
    assert!(p.receipt().requests.is_empty());
    let mut s = manager(&f, std::slice::from_ref(&p), limits(&f));
    let t = s
        .admit(&admissions(std::slice::from_ref(&p)))
        .unwrap()
        .remove(0);
    complete(&mut s, std::slice::from_ref(&t));
    let held = s.sources(&t).unwrap();
    s.remove(&t).unwrap();
    let state = s.poll(4).unwrap();
    assert_eq!(state.usage.outstanding, 0);
    assert_eq!(state.usage.retained_plans, 1);
    assert_eq!(state.usage.retired_slots, 1);
    assert!(held.plan().is_err());
    assert!(s.admit(&admissions(std::slice::from_ref(&p))).is_err());
    drop(held);
    until(|| s.poll(4).unwrap().usage.retired_slots == 0);
    s.admit(&admissions(std::slice::from_ref(&p))).unwrap();
}
struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
#[test]
fn controlled_cancellation_before_and_after_extraction_preserves_other_cells() {
    for after in [false, true] {
        let f = Fixture::new();
        let p = f.plans();
        let mut s = manager(&f, &p, limits(&f));
        let t = s.admit(&admissions(&p)).unwrap();
        let pause = Pause::new(after);
        s.slots[1].host.as_mut().unwrap().pause = Some(pause.clone());
        let _release = Release(pause.clone());
        s.poll(2).unwrap();
        pause.reached();
        s.remove(&t[1]).unwrap();
        assert!(t[1].check().is_err());
        assert_eq!(s.snapshot().usage.retired_slots, 1);
        pause.release();
        pause.completed();
        until(|| {
            let state = s.poll(4).unwrap();
            state.usage.retired_slots == 0 && s.sources(&t[0]).is_ok() && s.sources(&t[2]).is_ok()
        });
        assert!(s.sources(&t[1]).is_err());
        assert_eq!(s.snapshot().usage.active_slots, 2);
        assert!(s.sources(&t[0]).unwrap().model(0).is_ok());
        assert!(s.sources(&t[2]).unwrap().model(0).is_ok());
        assert!(s.snapshot().usage.pinned_source_bytes <= s.limits.source_bytes);
    }
}
