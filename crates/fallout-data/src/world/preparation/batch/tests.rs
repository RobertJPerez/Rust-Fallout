use super::*;
use crate::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    resource_jobs::tests::{Fixture, Pause},
    store::RecordStore,
    vfs::MountIndex,
};
use std::{fs, sync::Arc, time::Instant};

struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
fn sub(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn record(tag: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        body,
    ]
    .concat()
}
fn group(kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &0x200u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn setup(count: usize) -> (Fixture, RecordStore, MountIndex, CellModelPlan) {
    let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    nif.extend(0x14020007u32.to_le_bytes());
    nif.push(1);
    for value in [11u32, 0, 34] {
        nif.extend(value.to_le_bytes());
    }
    nif.extend([0; 21]);
    let fixture = Fixture::new(&nif, true);
    let mut sources = record(
        b"TES4",
        0,
        &sub(
            b"HEDR",
            &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    let mut members = Vec::new();
    let mut mounts = MountIndex::default();
    for index in 0..count {
        let file = [b'a' + index as u8, b'.', b'n', b'i', b'f'];
        let mut bsa = fs::read(&fixture.source_member.container).unwrap();
        bsa[76..81].copy_from_slice(&file);
        let path = fixture.source.path().join(format!("source{index}.bsa"));
        fs::write(&path, bsa).unwrap();
        NvArchive::open(&path).unwrap().census(&mut mounts).unwrap();
        sources.extend(record(
            b"STAT",
            0x400 + index as u32,
            &sub(b"MODL", &[file.as_slice(), &[0]].concat()),
        ));
        members.extend(record(
            b"REFR",
            0x300 + index as u32,
            &[
                sub(b"NAME", &(0x400 + index as u32).to_le_bytes()),
                sub(b"DATA", &[0; 24]),
            ]
            .concat(),
        ));
    }
    sources.extend(record(
        b"CELL",
        0x200,
        &[sub(b"EDID", b"PreparedClinic\0"), sub(b"DATA", &[1])].concat(),
    ));
    sources.extend(group(6, &group(9, &members)));
    fs::write(fixture.source.path().join("FalloutNV.esm"), sources).unwrap();
    let mut store = RecordStore::open_nv_headers(
        fixture.source.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let root = FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: 0x200,
    };
    let plan = CellModelPlan::load(&mut store, &root, &mounts, Default::default()).unwrap();
    (fixture, store, mounts, plan)
}
fn drained(preparation: &CellPreparation) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while preparation.usage().outstanding != 0 {
        assert!(
            Instant::now() < deadline,
            "old worker reservations did not drain"
        );
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(preparation.usage().decoded_bytes, 0);
}

#[test]
fn whole_cell_replacement_revokes_controlled_running_and_queued_members() {
    for after_extract in [false, true] {
        let (fixture, mut store, mounts, plan) = setup(2);
        let mut report =
            crate::world::inspect_cell(&mut store, b"PreparedClinic", &mounts).unwrap();
        let original = serde_json::to_vec(&report).unwrap();
        let mut preparation = CellPreparation::new(
            plan.clone(),
            fixture.source.path(),
            Some(fixture.cache.path()),
            resource_jobs::Limits {
                workers: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let pause = Pause::new(after_extract);
        preparation.pause = Some(pause.clone());
        let _release = Release(pause.clone());
        assert!(!preparation.poll().unwrap());
        pause.reached();
        assert_eq!(preparation.usage().outstanding, 2);
        assert!(preparation.take_ready().unwrap().is_none());
        preparation.replace(plan).unwrap();
        pause.release();
        pause.completed();
        drained(&preparation);
        assert_eq!(fs::read_dir(fixture.cache.path()).unwrap().count(), 0);
        assert_eq!(serde_json::to_vec(&report).unwrap(), original);
        preparation.pause = None;
        let receipt = preparation
            .wait()
            .unwrap()
            .publish_into(&mut report)
            .unwrap();
        assert_eq!(receipt.generation, 2);
        assert_eq!(report.model_probes.len(), 2);
        drained(&preparation);
    }
}

#[test]
fn whole_cell_retry_revokes_completed_unconsumed_worker_payload_but_reuses_valid_cache() {
    let (fixture, mut store, mounts, plan) = setup(1);
    let mut report = crate::world::inspect_cell(&mut store, b"PreparedClinic", &mounts).unwrap();
    let mut preparation = CellPreparation::new(
        plan,
        fixture.source.path(),
        Some(fixture.cache.path()),
        Default::default(),
    )
    .unwrap();
    let pause = Pause::new(true);
    preparation.pause = Some(pause.clone());
    let _release = Release(pause.clone());
    preparation.poll().unwrap();
    pause.reached();
    pause.release();
    pause.completed();
    assert_eq!(preparation.usage().outstanding, 1);
    assert!(report.model_probes.is_empty());
    preparation.retry().unwrap();
    drained(&preparation);
    preparation.pause = None;
    let receipt = preparation
        .wait()
        .unwrap()
        .publish_into(&mut report)
        .unwrap();
    assert_eq!(receipt.generation, 2);
    assert!(report.model_probes[0].cache.as_ref().unwrap().reused);
    drained(&preparation);
}
